"""Build and qualify the selected Native management assembly through actual owner ports."""
import argparse
import hashlib
import json
from pathlib import Path
import secrets
import shutil
import socket
import sqlite3
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from evidence import native_build
from host import native_server
from verify_pg import sql


def private(path, value):
    path.write_text(value)
    path.chmod(0o600)


def toml_value(value):
    if isinstance(value, dict):
        return '{' + ','.join(json.dumps(key)+'='+toml_value(item) for key,item in value.items() if item is not None) + '}'
    if isinstance(value, list):
        return '[' + ','.join(toml_value(item) for item in value) + ']'
    if value is None:
        raise ValueError('TOML config must omit optional null values')
    return json.dumps(value)


def configuration(source, plugin, values):
    directory = source/'plugins'/plugin
    directory.mkdir(exist_ok=True)
    (directory/'default.disabled').unlink(missing_ok=True)
    (directory/'default.toml').write_text('\n'.join(json.dumps(key)+' = '+toml_value(value)
        for key,value in values.items() if value is not None)+'\n')


def request(base, path, token=None, payload=None):
    headers = {'Content-Type':'application/json','User-Agent':'lenso-reference-qualification/1'}
    if token:
        headers['Authorization'] = 'Bearer '+token
    req = Request(base.rstrip('/')+path, data=None if payload is None else json.dumps(payload).encode(), headers=headers)
    try:
        response = urlopen(req, timeout=30)
    except HTTPError as error:
        response = error
    with response:
        return response.status, json.load(response)


def rpc(uri, token, method, parameters=None, session=None, identifier=1):
    packet={'jsonrpc':'2.0','method':method}
    if identifier is not None:
        packet['id']=identifier
    if parameters is not None:
        packet['params']=parameters
    headers={'Content-Type':'application/json','Accept':'application/json, text/event-stream',
             'Authorization':'Bearer '+token,'MCP-Protocol-Version':'2025-11-25'}
    if session:
        headers['Mcp-Session-Id']=session
    try:
        response=urlopen(Request(uri,data=json.dumps(packet).encode(),headers=headers),timeout=30)
    except HTTPError as error:
        response=error
    with response:
        session=response.headers.get('Mcp-Session-Id',session)
        if response.status==202:
            return response.status,{},session
        if 'text/event-stream' in response.headers.get('Content-Type',''):
            while line:=response.readline():
                if line.startswith(b'data:'):
                    return response.status,json.loads(line[5:]),session
            raise ValueError('MCP ended without a response')
        raw=response.read()
        if response.status>=400:
            try:
                packet = json.loads(raw)
            except (json.JSONDecodeError, UnicodeDecodeError):
                packet = {'transport_status': response.status,
                    'authentication_required': raw == b'Management credential required'}
            return response.status, packet, session
        return response.status,json.loads(raw),session


def mcp_owner(packet):
    result = packet['result']
    assert not result.get('isError', False), result
    return result.get('structuredContent') or json.loads(next(
        block['text'] for block in result['content'] if block['type'] == 'text'))


def mcp_denied(status, packet):
    if status == 401 and packet.get('authentication_required'):
        return True
    if status != 200:
        return False
    if packet.get('error', {}).get('message') == 'Management permission denied':
        return True
    result = packet.get('result', {})
    return result.get('isError', False) and any(block.get('type') == 'text'
        and block.get('text') == 'Management permission denied' for block in result.get('content', []))


def prove_mcp(uri, token, *, allow_write=False, revision=1):
    status,initialized,session=rpc(uri,token,'initialize',{'protocolVersion':'2025-11-25',
        'capabilities':{},'clientInfo':{'name':'lenso-ops-qualification','version':'1.0.0'}})
    assert status==200 and initialized['result']['protocolVersion']=='2025-11-25',initialized
    assert rpc(uri,token,'notifications/initialized',session=session,identifier=None)[0]==202
    status,catalog,session=rpc(uri,token,'tools/list',{},session)
    assert status==200 and 'error' not in catalog,catalog
    tools=catalog['result']['tools']
    reads=[tool for tool in tools if tool['description']=='Read the primary state']
    assert len(reads)==1 and len(tools)==(3 if allow_write else 2),tools
    assert all('approve' not in tool['name'] and 'token' not in tool['name'] for tool in tools)
    status,result,_=rpc(uri,token,'tools/call',{'name':reads[0]['name'],'arguments':{'input':{}}},session)
    assert status==200 and 'error' not in result and not result['result'].get('isError',False),result
    owner=mcp_owner(result)
    assert owner['state']=='succeeded' and json.loads(owner['result_json'])['revision']==revision,owner
    return session, reads[0]['name']


def prove_mcp_write(uri, url, tokens, phase=None):
    session, _ = prove_mcp(uri, tokens['alice'], allow_write=True)
    status, packet, _ = rpc(uri, tokens['alice'], 'tools/list', {}, session)
    assert status == 200 and 'error' not in packet, packet
    write = next(tool for tool in packet['result']['tools']
                 if tool['description'] == 'Change the primary state at the reviewed revision')
    assert {'input', 'idempotency_key'} <= set(write['inputSchema']['required']), write
    arguments = {'input': {'value': 61}, 'idempotency_key': 'mcp-reviewed-write', 'expected_revision': '1'}
    call = {'name': write['name'], 'arguments': arguments}
    if phase is not None:
        private(phase/'mcp-write-started.json', json.dumps({'idempotency_key': arguments['idempotency_key']}))
    status, packet, _ = rpc(uri, tokens['alice'], 'tools/call', call, session)
    assert status == 200 and 'error' not in packet, packet
    pending = mcp_owner(packet)
    assert pending['state'] == 'pending_approval', pending
    operation = pending['operation_id']
    if phase is not None:
        private(phase/'mcp-pending.json', json.dumps({'operation_id': operation, 'state': pending['state']}))
    _, observed, _ = rpc(uri, tokens['alice'], 'tools/call',
        {'name': 'management__status', 'arguments': {'operation_id': operation}}, session)
    assert mcp_owner(observed)['state'] == 'pending_approval', observed
    status, intent = request(url, '/human-management/intents/'+operation, tokens['bob'])
    assert status == 200 and intent['requester'] == 'alice' and json.loads(intent['parameters_json']) == {
        'input': {'value': 61}, 'expected_revision': '1'}, intent
    decision = {'operation_id': operation, 'intent_digest': intent['intent_digest'], 'decision': 'approved'}
    assert request(url, '/human-management/decide', tokens['alice'], decision)[0] == 403
    assert request(url, '/human-management/decide', tokens['machine'], decision)[0] == 403
    status, approved = request(url, '/human-management/decide', tokens['bob'], decision)
    assert status == 200 and approved['status'] == 'approved' and not approved['audit_pending'], approved
    if phase is not None:
        private(phase/'mcp-approved.json', json.dumps({'operation_id': operation, 'status': approved['status']}))
    status, packet, _ = rpc(uri, tokens['alice'], 'tools/call', call, session)
    assert status == 200 and 'error' not in packet, packet
    committed = mcp_owner(packet)
    assert committed['state'] == 'succeeded' and json.loads(committed['result_json'])['revision'] == 2, committed
    if phase is not None:
        private(phase/'mcp-committed.json', json.dumps({'operation_id': operation, 'receipt': committed}))
    assert mcp_owner(rpc(uri, tokens['alice'], 'tools/call', call, session)[1]) == committed
    _, conflict, _ = rpc(uri, tokens['alice'], 'tools/call',
        {**call, 'arguments': {**arguments, 'input': {'value': 62}}}, session)
    assert conflict.get('error') or conflict['result'].get('isError', False), conflict
    return operation, committed


def resume_committed(cli, root, receipt, *, allow_write=False):
    """Reconcile the known terminal operation, then qualify only the uncompleted phases."""
    assert not receipt.exists() and not (root/'project').exists()
    assert (root/'tool-audience-build-completed.json').exists()
    phase=root/'qualification-after-tool-audience'
    phase.mkdir(mode=0o700)
    key='reviewed-assertion-reconciled-1'
    with sqlite3.connect('file:'+str(root/'management.sqlite')+'?mode=ro',uri=True) as db:
        rows=db.execute('SELECT operation_id,response_json FROM management_invocations WHERE idempotency_key=?',(key,)).fetchall()
    assert len(rows)==1
    operation, raw=rows[0];committed=json.loads(raw)
    assert committed['operation_id']==operation and committed['state']=='succeeded'
    assert json.loads(committed['result_json'])['revision']==1
    private(phase/'prior-committed-metadata.json',json.dumps({'operation_id':operation,'receipt':committed}))
    setup=json.loads((root/'setup.json').read_text()); selections=json.loads((root/'operator-input.json').read_text())
    tokens={name:(root/f'tokens/{name}.secret').read_text() for name in setup['tokens']}
    artifact=root/'distribution';facilities=root/'facilities.json';mcp_uri=selections['resource_uri']
    evidence={'schema':'lenso.ops-management-qualification.v1','target':'native_http','real_database':True,
        'profile':'operators-api-token','build':native_build(artifact),'cli_sha256':hashlib.sha256(cli.read_bytes()).hexdigest(),
        'source_deleted':True,'human_session_and_pat':'not_run','workers_management':'not_run',
        'prior_completed_phase':{'build':native_build(root/'distribution-before-tool-audience'),
            'operation_id':operation,'value':47,'revision':1,'reconciled_through_current_owner':True},'cases':[]}
    with native_server(cli,artifact,facilities,root) as url:
        assert request(url,'/management/operations/'+operation,tokens['alice'])==(200,committed)
        command={'entry_id':'state.update','version':'2.0.0','input_json':'{"value":47}',
            'idempotency_key':key,'expected_revision':'0'}
        assert request(url,'/management/invoke',tokens['alice'],command)==(200,committed)
        assert request(url,'/management/invoke',tokens['alice'],{**command,'input_json':'{"value":48}'})[0]==409
        status,catalog=request(url,'/management-tools/catalog',tokens['alice'])
        assert status==200 and len(catalog['tools'])==3
        assert all('approve' not in item['name'] and 'token' not in item['name'] for item in catalog['tools'])
        read_tool=next(item for item in catalog['tools'] if item['description']=='Read the primary state')
        status,read=request(url,'/management-tools/execute',tokens['alice'],
            {'name':read_tool['name'],'arguments_json':'{"input":{}}'})
        assert status==200 and json.loads(json.loads(read['content'])['result_json'])['revision']==1
        evidence['cases']+=['reconciled_terminal_intent','same_intent_has_single_business_receipt','duplicate_intent_conflict',
            'agent_no_human_or_credential_tools','actual_agent_typed_owner_read']
        prove_mcp(mcp_uri,tokens['alice'],allow_write=allow_write)
        evidence['cases'].append('actual_bound_mcp_catalog_and_read' if allow_write else 'actual_bound_read_only_mcp')
        if allow_write:
            mcp_operation,mcp_committed=prove_mcp_write(mcp_uri,url,tokens,phase)
            evidence['cases']+=['actual_mcp_pending_write','mcp_self_and_machine_approval_denied',
                'mcp_distinct_human_approval','mcp_single_business_receipt','mcp_duplicate_intent_conflict']
    with native_server(cli,artifact,facilities,root) as url:
        assert request(url,'/management/operations/'+operation,tokens['alice'])==(200,committed)
        if allow_write:
            assert request(url,'/management/operations/'+mcp_operation,tokens['alice'])==(200,mcp_committed)
        session,read_tool=prove_mcp(mcp_uri,tokens['alice'],allow_write=allow_write,revision=2 if allow_write else 1)
        token_id=setup['tokens']['alice']['token_id']
        private(phase/'revoke-started.json',json.dumps({'token_id':token_id}))
        subprocess.run([str(root/'operator'),'revoke',str(root/'operator-input.json'),token_id],check=True)
        private(phase/'revoke-completed.json','{"state":"completed"}\n')
        assert request(url,'/management/catalog',tokens['alice'])[0] in [401,403]
        assert request(url,'/management-tools/catalog',tokens['alice'])[0]==403
        assert mcp_denied(*rpc(mcp_uri,tokens['alice'],'tools/list',{},session)[:2])
        assert mcp_denied(*rpc(mcp_uri,tokens['alice'],'tools/call',{'name':read_tool,'arguments':{'input':{}}},session)[:2])
        prove_mcp(mcp_uri,tokens['bob'],allow_write=allow_write,revision=2 if allow_write else 1)
        evidence['cases']+=['mcp_session_does_not_cache_revoked_authority','unrelated_current_mcp_session_remains_available']
    domain=json.loads((mcp_committed if allow_write else committed)['result_json'])
    evidence.update(final_business_state={'value':domain['value'],'revision':domain['revision']},
        persistent_restart='passed',fresh_credential_revocation='passed',shutdown='passed')
    receipt.write_text(json.dumps(evidence,indent=2)+'\n');private(phase/'completed.json','{"state":"completed"}\n')
    print(json.dumps({'receipt':str(receipt),'cases':evidence['cases']}))


def profile(source, root, setup, operator_input, mcp_uri, allow_write=False):
    owners = {'auth':'lenso.auth.api-token','access':'lenso.access-control.postgres',
              'approval':'lenso.business-approval.postgres','audit':'lenso.audit-log.postgres'}
    for plugin in ['lenso.auth.account','lenso.auth.password','lenso.auth.human-api-token']:
        directory = source/'plugins'/plugin
        if directory.exists():
            shutil.rmtree(directory)
    for name, plugin in owners.items():
        configuration(source, plugin, setup['owners'][name])
    for plugin, values in {
        'example.ops-management':{'deployment':operator_input['deployment'],'target_instance':'example.ops-state/primary','read_only':False},
        'example.ops-management-web':{'credential_scheme':'bearer','human_login':False},
        'example.ops-management-mcp':{'allow_state_update':allow_write},
        'example.ops-management-tools':{}, 'example.ops-secrets':{},
        'lenso.agent.management-tools':{}, 'example.ops-bootstrap':{},
    }.items():
        configuration(source, plugin, values)
    configuration(source,'example.ops-web',{})
    (source/'plugins/example.ops-web/default.toml').unlink()
    (source/'plugins/example.ops-web/default.disabled').touch()
    choices=[]
    def bind(consumer, requirement, provider, key='default'):
        choices.append({'consumer':{'plugin_id':consumer,'instance_key':'default'},'requirement_id':requirement,
                        'provider':{'plugin_id':provider,'instance_key':key}})
    bind('example.ops-management','state','example.ops-state','primary')
    for requirement, provider in [('credential_state',owners['auth']),('access',owners['access']),
                                  ('approval',owners['approval']),('audit',owners['audit'])]:
        bind('example.ops-management',requirement,provider)
    for consumer in ['example.ops-management-web','example.ops-management-mcp','example.ops-management-tools','example.ops-bootstrap']:
        bind(consumer,'auth',owners['auth'])
    for consumer in ['example.ops-management-web','example.ops-management-mcp']:
        bind(consumer,'management','example.ops-management')
    bind('lenso.agent.management-tools','management','example.ops-management')
    bind('example.ops-management-web','human','example.ops-management')
    choices.append({'consumer':{'plugin_id':'example.ops-management-web','instance_key':'default'},
                    'requirement_id':'delegation','provider':None})
    bind('example.ops-management-tools','tools','lenso.agent.management-tools')
    bind('example.ops-bootstrap','access_admin',owners['access'])
    choices.sort(key=lambda choice:(choice['consumer']['plugin_id'],choice['consumer']['instance_key'],choice['requirement_id']))
    (source/'plugins/.dependencies.json').write_text(json.dumps({'schema_version':1,'choices':choices},indent=2)+'\n')
    references={}
    for name, owner in owners.items():
        references[name+'/database']={'path':str(root/('audit-uri.secret' if name=='audit' else 'database-uri.secret')),
                                     'callers':[owner+'/default']}
    for name in ['signing','pepper']:
        references['auth/'+name]={'path':str(root/(name+'.secret')),'callers':[owners['auth']+'/default']}
    grants={'example.ops-management/default':{'authority':{
        'deployment':operator_input['deployment'],'journal':str(root/'management.sqlite'),
        'qualification':str(root/'qualification.sqlite'),'issuer':operator_input['issuer'],
        'public_key':setup['public_key'],'max_assertion_ttl_seconds':60}},
        'example.ops-secrets/default':{'secrets':references},
        'example.ops-management-mcp/default':{'transport':{'listen':mcp_uri.removeprefix('http://').removesuffix('/mcp'),'resource_uri':mcp_uri}},
        'example.ops-bootstrap/default':{'bootstrap':{'subject':'bootstrap-human','bootstrap_token_file':str(root/'tokens/bootstrap.secret'),
            'scopes':[{'kind':'ops-state','id':'primary-state', 'roles':[{
                'id':'operators','permissions':['ops.state.read','ops.state.update','management.approval.read','management.approval.decide'],
                'subjects':['alice','bob']}]}]}}}
    for instance in ['primary','secondary']:
        config=source/f'plugins/example.ops-state/{instance}.toml'
        config.write_text(config.read_text().replace('simulated','native-pg'))
        grants['example.ops-state/'+instance]={'state':{'profile':'native-pg',
            'connection_uri_file':str(root/'database-uri.secret'),'schema':operator_input['schemas']['auth']+'_'+instance}}
    facilities=root/'facilities.json'
    private(facilities,json.dumps({'schema':'lenso.host-facilities.v1','instances':grants},indent=2)+'\n')
    return facilities


def prove(url, tokens, *, idempotency_key='reviewed-1'):
    alice, bob = tokens['alice'],tokens['bob']
    assert request(url,'/management/catalog')[0] == 401
    status,catalog=request(url,'/management/catalog',alice)
    assert status==200 and {entry['id'] for entry in catalog['entries']}=={'state.read','state.update'}, catalog
    for name in ['machine','wrong-deployment']:
        status, denied=request(url,'/management/catalog',tokens[name])
        assert status==403 or (status==200 and denied['entries']==[]), (name,denied)
    command={'entry_id':'state.read','version':'2.0.0','input_json':'{}','idempotency_key':None,'expected_revision':None}
    status,read=request(url,'/management/invoke',alice,command)
    assert status==200 and read['state']=='succeeded' and json.loads(read['result_json'])['revision']==0,read
    command.update(entry_id='state.update',input_json='{"value":47}',idempotency_key=idempotency_key,expected_revision='0')
    status,pending=request(url,'/management/invoke',alice,command)
    assert status==200 and pending['state']=='pending_approval',pending
    operation=pending['operation_id']
    assert request(url,'/state/primary',alice,{'value':99,'expected_revision':0,'idempotency_key':'bypass'})[0] == 404
    status,intent=request(url,'/human-management/intents/'+operation,bob)
    assert status==200 and intent['requester']=='alice' and json.loads(intent['parameters_json'])=={
        'input':{'value':47},'expected_revision':'0'},intent
    decision={'operation_id':operation,'intent_digest':intent['intent_digest'],'decision':'approved'}
    assert request(url,'/human-management/decide',alice,decision)[0]==403
    assert request(url,'/human-management/decide',tokens['machine'],decision)[0]==403
    assert request(url,'/human-management/decide',bob,{**decision,'intent_digest':'0'*64})[0]==409
    status,approved=request(url,'/human-management/decide',bob,decision)
    assert status==200 and approved['status']=='approved' and approved['audit_pending'] is False,approved
    status,committed=request(url,'/management/invoke',alice,command)
    assert status==200 and committed['state']=='succeeded' and json.loads(committed['result_json'])['revision']==1,committed
    assert request(url,'/management/invoke',alice,command)==(200,committed)
    assert request(url,'/management/invoke',alice,{**command,'input_json':'{"value":48}'})[0]==409
    status,tools=request(url,'/management-tools/catalog',alice)
    assert status==200 and tools['tools'] and all('approve' not in item['name'] and 'token' not in item['name'] for item in tools['tools']),tools
    read_tool=next(item for item in tools['tools'] if item['description']=='Read the primary state')
    status,tool_read=request(url,'/management-tools/execute',alice,{'name':read_tool['name'],'arguments_json':'{"input":{}}'})
    assert status==200 and json.loads(json.loads(tool_read['content'])['result_json'])['revision']==1,tool_read
    return operation,committed,['current_operators_catalog','machine_and_wrong_deployment_denied','ordinary_typed_owner_read',
        'immutable_pending_intent','direct_business_write_disabled','self_and_machine_approval_denied','digest_mismatch_denied',
        'distinct_human_approval','single_business_receipt','duplicate_intent_conflict','agent_no_human_or_credential_tools','actual_agent_typed_owner_read']


def qualify_existing(cli, root, receipt, *, allow_write=False):
    """Continue a built, enrolled App after a known pre-dispatch fixture assertion failure."""
    assert not (root/'project').exists() and (root/'distribution').is_dir()
    assert (root/'bootstrap-started.json').is_file() and not receipt.exists()
    phase=root/'qualification-after-assertion'
    phase.mkdir(mode=0o700)
    private(phase/'started.json',json.dumps({'reason':'immutable intent includes input and expected_revision',
        'prior_operation_dispatched':False,'new_request_key':'reviewed-assertion-reconciled-1'}))
    selections=json.loads((root/'operator-input.json').read_text())
    setup=json.loads((root/'setup.json').read_text())
    tokens={name:(root/f'tokens/{name}.secret').read_text() for name in setup['tokens']}
    artifact=root/'distribution';facilities=root/'facilities.json';mcp_uri=selections['resource_uri']
    evidence={'schema':'lenso.ops-management-qualification.v1','target':'native_http','real_database':True,
        'build':native_build(artifact),'cli_sha256':hashlib.sha256(cli.read_bytes()).hexdigest(),
        'profile':'operators-api-token','human_session_and_pat':'not_run','workers_management':'not_run',
        'source_deleted':True,'reconciliation':'new explicit request after retained pre-dispatch assertion failure'}
    with native_server(cli,artifact,facilities,root) as url:
        operation,committed,evidence['cases']=prove(url,tokens,idempotency_key='reviewed-assertion-reconciled-1')
        private(phase/'business-completed.json',json.dumps({'operation':operation,'receipt':committed}))
        prove_mcp(mcp_uri,tokens['alice'],allow_write=allow_write)
        evidence['cases'].append('actual_bound_mcp_catalog_and_read' if allow_write else 'actual_bound_read_only_mcp')
        if allow_write:
            mcp_operation,mcp_committed=prove_mcp_write(mcp_uri,url,tokens)
            private(phase/'mcp-business-completed.json',json.dumps({'operation':mcp_operation,'receipt':mcp_committed}))
            evidence['cases'].extend(['actual_mcp_pending_write','mcp_self_and_machine_approval_denied',
                'mcp_distinct_human_approval','mcp_single_business_receipt','mcp_duplicate_intent_conflict'])
        domain=json.loads((mcp_committed if allow_write else committed)['result_json'])
        evidence['final_business_state']={'value':domain['value'],'revision':domain['revision']}
    with native_server(cli,artifact,facilities,root) as url:
        assert request(url,'/management/operations/'+operation,tokens['alice'])==(200,committed)
        if allow_write:
            assert request(url,'/management/operations/'+mcp_operation,tokens['alice'])==(200,mcp_committed)
        session,read_tool=prove_mcp(mcp_uri,tokens['alice'],allow_write=allow_write,revision=2 if allow_write else 1)
        private(phase/'revoke-started.json',json.dumps({'token_id':setup['tokens']['alice']['token_id']}))
        subprocess.run([str(root/'operator'),'revoke',str(root/'operator-input.json'),setup['tokens']['alice']['token_id']],check=True)
        private(phase/'revoke-completed.json','{"state":"completed"}\n')
        assert request(url,'/management/catalog',tokens['alice'])[0] in [401,403]
        assert request(url,'/management-tools/catalog',tokens['alice'])[0]==403
        status,result,_=rpc(mcp_uri,tokens['alice'],'tools/list',{},session)
        assert mcp_denied(status,result), 'Revoked MCP session did not return a known authorization denial'
        status,result,_=rpc(mcp_uri,tokens['alice'],'tools/call',{'name':read_tool,'arguments':{'input':{}}},session)
        assert mcp_denied(status,result), 'Revoked MCP invocation did not return a known authorization denial'
        prove_mcp(mcp_uri,tokens['bob'],allow_write=allow_write,revision=2 if allow_write else 1)
        evidence['cases'].extend(['mcp_session_does_not_cache_revoked_authority','unrelated_current_mcp_session_remains_available'])
    evidence.update(persistent_restart='passed',fresh_credential_revocation='passed',shutdown='passed')
    receipt.write_text(json.dumps(evidence,indent=2)+'\n')
    private(phase/'completed.json','{"state":"completed"}\n')
    print(json.dumps({'receipt':str(receipt),'cases':evidence['cases']}))


def finish_known_revocation(cli,root,receipt,*,allow_write=False):
    phase=root/'qualification-after-tool-audience'
    assert not receipt.exists() and (phase/'revoke-completed.json').exists()
    assert not (phase/'revocation-assertion-reconciled.json').exists()
    prior=json.loads((phase/'prior-committed-metadata.json').read_text())
    committed=prior['receipt'];setup=json.loads((root/'setup.json').read_text())
    tokens={name:(root/f'tokens/{name}.secret').read_text() for name in setup['tokens']}
    mcp_uri=json.loads((root/'operator-input.json').read_text())['resource_uri']
    final=json.loads((phase/'mcp-committed.json').read_text())['receipt'] if allow_write else committed
    domain=json.loads(final['result_json'])
    with native_server(cli,root/'distribution',root/'facilities.json',root) as url:
        status,denied=request(url,'/management/catalog',tokens['alice'])
        assert status in [401,403] and denied.get('error')!='unavailable'
        assert request(url,'/management-tools/catalog',tokens['alice'])[0]==403
        status,denied,_=rpc(mcp_uri,tokens['alice'],'initialize',{'protocolVersion':'2025-11-25',
            'capabilities':{},'clientInfo':{'name':'revocation-reconciliation','version':'1.0.0'}})
        assert mcp_denied(status,denied)
        prove_mcp(mcp_uri,tokens['bob'],allow_write=allow_write,revision=domain['revision'])
        assert request(url,'/management/catalog',tokens['bob'])[0]==200
        assert request(url,'/management-tools/catalog',tokens['bob'])[0]==200
    evidence={'schema':'lenso.ops-management-qualification.v1','target':'native_http','real_database':True,
        'profile':'operators-api-token','build':native_build(root/'distribution'),
        'cli_sha256':hashlib.sha256(cli.read_bytes()).hexdigest(),'source_deleted':True,
        'prior_completed_phase':{'build':native_build(root/'distribution-before-tool-audience'),
            'operation_id':prior['operation_id'],'value':47,'revision':1,'reconciled_through_current_owner':True},
        'assertion_reconciliation':{'invalid_credential_http_status':status,'revoke_repeated':False},
        'cases':['reconciled_terminal_intent','same_intent_has_single_business_receipt','duplicate_intent_conflict',
            'agent_no_human_or_credential_tools','actual_agent_typed_owner_read','actual_bound_mcp_catalog_and_read',
            'mcp_session_does_not_cache_revoked_authority','unrelated_current_mcp_session_remains_available'],
        'final_business_state':{'value':domain['value'],'revision':domain['revision']},
        'persistent_restart':'passed','fresh_credential_revocation':'passed','shutdown':'passed',
        'human_session_and_pat':'not_run','workers_management':'not_run'}
    if allow_write:
        evidence['cases']+=['actual_mcp_pending_write','mcp_self_and_machine_approval_denied',
            'mcp_distinct_human_approval','mcp_single_business_receipt','mcp_duplicate_intent_conflict']
    receipt.write_text(json.dumps(evidence,indent=2)+'\n')
    private(phase/'revocation-assertion-reconciled.json','{"state":"completed","revoke_repeated":false}\n')
    print(json.dumps({'receipt':str(receipt),'state':'passed'}))


def run():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli',type=Path,required=True)
    parser.add_argument('--database-url-file',type=Path,required=True)
    parser.add_argument('--audit-url-file',type=Path,required=True)
    parser.add_argument('--receipt',type=Path,required=True)
    parser.add_argument('--directory',type=Path,help='New task-owned directory, retained on failure')
    parser.add_argument('--source',type=Path,help='Already frozen source snapshot to use instead of copying')
    parser.add_argument('--resume-build',action='store_true',help='Resume only failed initial compilation after completed explicit setup')
    parser.add_argument('--mcp-write',action='store_true',help='Select the single reviewed state.update MCP entry for this test App')
    args=parser.parse_args();fixture=Path(__file__).parent.resolve()
    root=args.directory.resolve() if args.directory else Path(tempfile.mkdtemp(prefix='lenso-ops-management-'))
    root.mkdir(exist_ok=True)
    source=root/'project'
    if args.source:
        source=args.source.resolve(strict=True)
    else:
        shutil.copytree(fixture/'project',source,ignore=shutil.ignore_patterns('.lenso','target','dist'))
        shutil.copytree(fixture/'storage',root/'storage',ignore=shutil.ignore_patterns('target'))
    if not args.resume_build:
        private(root/'database-uri.secret',args.database_url_file.read_text().strip())
        private(root/'audit-uri.secret',args.audit_url_file.read_text().strip())
        private(root/'signing.secret',secrets.token_urlsafe(48));private(root/'pepper.secret',secrets.token_urlsafe(48))
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1',0));mcp_uri=f'http://127.0.0.1:{reservation.getsockname()[1]}/mcp'
    prefix='ops_management_'+str(secrets.randbelow(1_000_000_000))
    selections={'database_uri_file':str(root/'database-uri.secret'),'audit_uri_file':str(root/'audit-uri.secret'),
        'directory':str(root),'schemas':{name:prefix+'_'+name for name in ['auth','access','approval']},
        'deployment':prefix,'issuer':prefix+'.operators','signing_file':str(root/'signing.secret'),
        'pepper_file':str(root/'pepper.secret'),'resource_uri':mcp_uri}
    input_file=root/'operator-input.json'
    if args.resume_build:
        if not args.directory or not args.source or (root/'bootstrap-started.json').exists():
            parser.error('--resume-build requires the original directory/source before any bootstrap startup')
        selections=json.loads(input_file.read_text());mcp_uri=selections['resource_uri']
    else:
        private(input_file,json.dumps(selections))
    result=subprocess.check_output(['cargo','build','--locked','--manifest-path',str(fixture/'operator/Cargo.toml'),
                                    '--message-format=json-render-diagnostics'],text=True)
    records=[json.loads(line) for line in result.splitlines() if line.startswith('{')]
    executable=next(Path(record['executable']) for record in records if record.get('reason')=='compiler-artifact'
        and record.get('executable') and record['target']['name']=='lenso-ops-reference-operator')
    operator=root/'operator';shutil.copy2(executable,operator)
    if not args.resume_build:
        subprocess.run([str(operator),'setup',str(input_file)],check=True)
    setup=json.loads((root/'setup.json').read_text());facilities=profile(source,root,setup,selections,mcp_uri,args.mcp_write)
    uri=(root/'database-uri.secret').read_text()
    for instance,initial in ([] if args.resume_build else [('primary',11),('secondary',29)]):
        schema=selections['schemas']['auth']+'_'+instance
        sql(uri,f'BEGIN; CREATE SCHEMA {schema}; SET search_path TO {schema};\n'+(fixture/'storage/native-pg/schema.sql').read_text()
            +f"\nINSERT INTO state VALUES(true,'{instance}-state',{initial},0); COMMIT;")
    artifact=root/'bootstrap-distribution'
    build=lambda out:subprocess.run([str(args.cli),'app','build','--root',str(source),'--out',str(out)],check=True)
    build(artifact)
    (root/'bootstrap-started.json').write_text(json.dumps({'deployment':selections['deployment']}))
    with native_server(args.cli,artifact,facilities,root,ready_marker='Explicit operators bootstrap completed'):
        pass
    (source/'plugins/example.ops-bootstrap/default.toml').unlink()
    (source/'plugins/example.ops-bootstrap/default.disabled').touch()
    grants=json.loads(facilities.read_text());del grants['instances']['example.ops-bootstrap/default'];private(facilities,json.dumps(grants))
    artifact=root/'distribution';build(artifact)
    evidence={'schema':'lenso.ops-management-qualification.v1','target':'native_http','real_database':True,
        'build':native_build(artifact),'cli_sha256':hashlib.sha256(args.cli.read_bytes()).hexdigest(),
        'profile':'operators-api-token','human_session_and_pat':'not_run','workers_management':'not_run'}
    tokens={name:(root/f'tokens/{name}.secret').read_text() for name in setup['tokens']}
    shutil.rmtree(source);shutil.rmtree(root/'storage',ignore_errors=True);evidence['source_deleted']=True
    with native_server(args.cli,artifact,facilities,root) as url:
        operation,committed,evidence['cases']=prove(url,tokens)
        prove_mcp(mcp_uri,tokens['alice'],allow_write=args.mcp_write)
        evidence['cases'].append('actual_bound_mcp_catalog_and_read' if args.mcp_write else 'actual_bound_read_only_mcp')
        if args.mcp_write:
            mcp_operation,mcp_committed=prove_mcp_write(mcp_uri,url,tokens)
            evidence['cases'].extend(['actual_mcp_pending_write','mcp_self_and_machine_approval_denied',
                'mcp_distinct_human_approval','mcp_single_business_receipt','mcp_duplicate_intent_conflict'])
        domain = json.loads((mcp_committed if args.mcp_write else committed)['result_json'])
        evidence['final_business_state'] = {'value': domain['value'], 'revision': domain['revision']}
    with native_server(args.cli,artifact,facilities,root) as url:
        assert request(url,'/management/operations/'+operation,tokens['alice'])==(200,committed)
        if args.mcp_write:
            assert request(url,'/management/operations/'+mcp_operation,tokens['alice'])==(200,mcp_committed)
        session,read_tool=prove_mcp(mcp_uri,tokens['alice'],allow_write=args.mcp_write,revision=2 if args.mcp_write else 1)
        subprocess.run([str(operator),'revoke',str(input_file),setup['tokens']['alice']['token_id']],check=True)
        assert request(url,'/management/catalog',tokens['alice'])[0]==403
        assert request(url,'/management-tools/catalog',tokens['alice'])[0]==403
        status,result,_=rpc(mcp_uri,tokens['alice'],'tools/list',{},session)
        assert mcp_denied(status, result), 'Revoked MCP session did not return a known authorization denial'
        status,result,_=rpc(mcp_uri,tokens['alice'],'tools/call',{'name':read_tool,'arguments':{'input':{}}},session)
        assert mcp_denied(status, result), 'Revoked MCP invocation did not return a known authorization denial'
        prove_mcp(mcp_uri, tokens['bob'], allow_write=args.mcp_write, revision=2 if args.mcp_write else 1)
        evidence['cases'].append('mcp_session_does_not_cache_revoked_authority')
        evidence['cases'].append('unrelated_current_mcp_session_remains_available')
    evidence.update(persistent_restart='passed',fresh_credential_revocation='passed',shutdown='passed')
    args.receipt.parent.mkdir(parents=True,exist_ok=True);args.receipt.write_text(json.dumps(evidence,indent=2)+'\n')
    print(json.dumps({'receipt':str(args.receipt),'cases':evidence['cases'],'persistent_restart':'passed'}))


if __name__=='__main__':
    run()
