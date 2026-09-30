"""Qualify actual Password/Account sessions and personal tokens in an ordinary Native App."""
import argparse
from datetime import datetime, timedelta, timezone
import hashlib
from http.cookies import SimpleCookie
import json
from pathlib import Path
import secrets
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from evidence import native_build
from host import native_server
from verify_management import configuration, private, toml_value
from verify_pg import sql


def call(url, path, session=None, payload=None, origin=None, csrf=True, bearer=None,
         expected_subject=None, binding=None):
    headers={'Content-Type':'application/json','User-Agent':'lenso-reference-qualification/1'}
    if origin:
        headers['Origin']=origin
    if session:
        headers['Cookie']='; '.join(name+'='+value for name,value in session.items())
        if csrf:
            headers['x-csrf-token']=session['__Host-lenso-csrf'] if csrf is True else str(csrf)
    if bearer:
        headers['Authorization']='Bearer '+bearer
    if expected_subject:
        headers['X-Lenso-Expected-Subject']=expected_subject
    if binding:
        headers.update(binding)
    try:
        response=urlopen(Request(url.rstrip('/')+path,headers=headers,
            data=None if payload is None else json.dumps(payload).encode()),timeout=30)
    except HTTPError as error:
        response=error
    with response:
        body=json.load(response)
        return response.status,body,response.headers


def login(url, origin, identifier, password):
    status,body,headers=call(url,'/auth/login',payload={'identifier':identifier,'password':password},origin=origin)
    assert status==200 and body=={'authenticated':True}, 'Actual Password.login failed'
    assert headers.get('Cache-Control')=='no-store'
    values={}
    for value in headers.get_all('Set-Cookie',[]):
        cookie=SimpleCookie();cookie.load(value)
        for name,item in cookie.items():
            assert item['secure'] and item['path']=='/' and item['samesite']=='Strict'
            if name=='__Host-lenso-session':
                assert item['httponly'], 'Account session cookie must be HttpOnly'
            values[name]=item.value
    assert set(values)=={'__Host-lenso-session','__Host-lenso-csrf'}, 'Expected explicit session and CSRF cookies'
    return values


def selected(source, plugin, key, config):
    directory=source/'plugins'/plugin;directory.mkdir(exist_ok=True)
    (directory/f'{key}.toml').write_text('\n'.join(json.dumps(name)+' = '+toml_value(value)
        for name,value in config.items() if value is not None)+'\n')


def profile(source, root, input, setup, human, native_port, origin):
    names={'auth':'lenso.auth.api-token','account':'lenso.auth.account','password':'lenso.auth.password',
        'access':'lenso.access-control.postgres','approval':'lenso.business-approval.postgres',
        'audit':'lenso.audit-log.postgres','human_facade':'lenso.auth.human-api-token'}
    for key,plugin in names.items():
        configuration(source,plugin,human['owners'][key])
    selected(source,names['access'],'human',human['owners']['human_access'])
    for plugin in ['example.ops-web','example.ops-bootstrap','example.ops-management-mcp']:
        directory=source/'plugins'/plugin;directory.mkdir(exist_ok=True)
        (directory/'default.toml').unlink(missing_ok=True);(directory/'default.disabled').touch()
    for plugin,config in {
        'example.ops-management':{'deployment':input['deployment'],'target_instance':'example.ops-state/primary','read_only':False},
        'example.ops-human-tokens':{'deployment':input['deployment']},
        'example.ops-management-web':{'credential_scheme':'session','human_login':True,'public_origin':origin},
        'example.ops-secrets':{},'example.ops-human-bootstrap':{},'example.ops-management-tools':{},
        'lenso.agent.management-tools':{},
        'lenso.web-ingress':{'bind_address':f'127.0.0.1:{native_port}', 'session_cookie':{
            'name':'__Host-lenso-session','csrf_cookie_name':'__Host-lenso-csrf','csrf_header_name':'x-csrf-token'}},
    }.items():
        configuration(source,plugin,config)
    selected(source,'example.ops-management','pat',{'deployment':input['deployment'],'target_instance':'example.ops-state/primary','read_only':False})
    choices=[]
    def bind(consumer, requirement, provider, key='default', consumer_key='default'):
        choices.append({'consumer':{'plugin_id':consumer,'instance_key':consumer_key},'requirement_id':requirement,
            'provider':{'plugin_id':provider,'instance_key':key}})
    for consumer,key,credential,access_key in [('example.ops-management','default',names['account'],'human'),
                                              ('example.ops-management','pat',names['auth'],'default')]:
        bind(consumer,'state','example.ops-state','primary',key)
        bind(consumer,'credential_state',credential,consumer_key=key)
        bind(consumer,'access',names['access'],access_key,key)
        for dependency in ['approval','audit']:
            bind(consumer,dependency,names[dependency],consumer_key=key)
    for requirement,provider in [('auth',names['account']),('management','example.ops-management'),
        ('human','example.ops-management'),('password',names['password']),('account_issuer',names['account']),
        ('human_tokens','example.ops-human-tokens')]:
        bind('example.ops-management-web',requirement,provider)
    choices.append({'consumer':{'plugin_id':'example.ops-management-web','instance_key':'default'},
                    'requirement_id':'delegation','provider':None})
    for requirement,provider,key in [('credential_state',names['account'],'default'),('access',names['access'],'human'),
        ('approval',names['approval'],'default'),('audit',names['audit'],'default'),('human_token_owner',names['human_facade'],'default')]:
        bind('example.ops-human-tokens',requirement,provider,key)
    for requirement,provider in [('auth',names['auth']),('password',names['password']),('account_issuer',names['account']),('access_admin',names['access'])]:
        bind('example.ops-human-bootstrap',requirement,provider)
    bind('example.ops-management-tools','auth',names['auth'])
    bind('example.ops-management-tools','tools','lenso.agent.management-tools')
    bind('lenso.agent.management-tools','management','example.ops-management','pat')
    for requirement,provider,key in [('account_state',names['account'],'default'),('api_tokens',names['auth'],'default'),('access',names['access'],'human')]:
        bind(names['human_facade'],requirement,provider,key)
    choices.sort(key=lambda choice:(choice['consumer']['plugin_id'],choice['consumer']['instance_key'],choice['requirement_id']))
    (source/'plugins/.dependencies.json').write_text(json.dumps({'schema_version':1,'choices':choices},indent=2)+'\n')
    references={}
    for key in ['auth','account','password','access','approval','audit']:
        callers=[names[key]+'/default']
        if key=='access':callers.append(names[key]+'/human')
        references[key+'/database']={'path':str(root/('audit-uri.secret' if key=='audit' else 'database-uri.secret')),'callers':callers}
    for key in ['auth','account']:
        for material in ['signing','pepper']:
            references[key+'/'+material]={'path':str(root/((material if key=='auth' else key+'-'+material)+'.secret')),'callers':[names[key]+'/default']}
    def authority(journal,issuer,key):
        return {'deployment':input['deployment'],'journal':str(root/journal),'qualification':str(root/'qualification.sqlite'),
            'issuer':issuer,'public_key':key,'max_assertion_ttl_seconds':60}
    grants={'example.ops-secrets/default':{'secrets':references},
        'example.ops-management/default':{'authority':authority('management.sqlite',human['account_issuer'],human['account_public_key'])},
        'example.ops-management/pat':{'authority':authority('pat-management.sqlite',input['issuer'],setup['public_key'])},
        'example.ops-human-tokens/default':{'authority':authority('human-tokens.sqlite',human['account_issuer'],human['account_public_key'])},
        'example.ops-human-bootstrap/default':{'bootstrap':{'deployment':input['deployment'],'bootstrap_subject':'bootstrap-human',
            'bootstrap_token_file':str(root/'tokens/bootstrap.secret'),'qualification':str(root/'qualification.sqlite'),
            'receipt':str(root/'human-enrollment.json'),'accounts':[{
                'alias':name,'identifier':name+'@ops.test','password_file':str(root/(name+'-password.secret')),
                'roles':[{'kind':'ops-state','id':'primary-state','role_id':'operators','permissions':[
                    'ops.state.read','ops.state.update','management.approval.read','management.approval.decide']},
                    {'kind':'management-deployment','id':input['deployment'],'role_id':'personal-tokens',
                     'permissions':['auth.pat.issue','auth.pat.list','auth.pat.revoke']}]} for name in ['alice','bob']]}}}
    for instance in ['primary','secondary']:
        path=source/f'plugins/example.ops-state/{instance}.toml'
        path.write_text(path.read_text().replace('simulated','native-pg'))
        grants['example.ops-state/'+instance]={'state':{'profile':'native-pg','connection_uri_file':str(root/'database-uri.secret'),
            'schema':input['schemas']['auth']+'_'+instance}}
    file=root/'facilities.json';private(file,json.dumps({'schema':'lenso.host-facilities.v1','instances':grants},indent=2)+'\n')
    return file


def prove(url, origin, root, deployment):
    enrollment=json.loads((root/'human-enrollment.json').read_text())['accounts']
    credentials={name:(root/(name+'-password.secret')).read_text() for name in ['alice','bob']}
    assert call(url,'/api/console/v1/session')[0]==401
    assert call(url,'/auth/login',payload={'identifier':'alice@ops.test','password':credentials['alice']},origin='https://wrong.example')[0]==403
    assert call(url,'/auth/login',payload={'identifier':'alice@ops.test','password':'wrong-password'},origin=origin)[0]==403
    sessions={name:login(url,origin,name+'@ops.test',credentials[name]) for name in credentials}
    for name,session in sessions.items():
        status,reply,headers=call(url,'/api/console/v1/session',session)
        assert status==200 and reply['subject']==enrollment[name]['subject'] and headers.get('Cache-Control')=='no-store'
    operator=[str(root/'operator')]
    subprocess.run([*operator,'unqualify',str(root/'operator-input.json'),enrollment['bob']['subject']],check=True)
    assert call(url,'/api/console/v1/session',sessions['bob'])[0]==403, 'Account login alone must not grant management qualification'
    subprocess.run([*operator,'qualify',str(root/'operator-input.json'),enrollment['bob']['subject']],check=True)
    command={'entry_id':'state.update','version':'2.0.0','input_json':'{"value":47}', 'idempotency_key':'human-reviewed-1','expected_revision':'0'}
    assert call(url,'/management/invoke',sessions['alice'],command,csrf=False)[0]==403
    assert call(url,'/management/invoke',sessions['alice'],command,csrf='mismatch')[0]==403
    status,pending,_=call(url,'/management/invoke',sessions['alice'],command)
    assert status==200 and pending['state']=='pending_approval'
    operation=pending['operation_id']
    status,intent,_=call(url,'/human-management/intents/'+operation,sessions['bob'])
    assert status==200 and intent['requester']==enrollment['alice']['subject']
    decision={'operation_id':operation,'intent_digest':intent['intent_digest'],'decision':'approved'}
    assert call(url,'/human-management/decide',sessions['alice'],decision)[0]==403
    status,approved,_=call(url,'/human-management/decide',sessions['bob'],decision)
    assert status==200 and approved['status']=='approved' and approved['audit_pending'] is False
    status,committed,_=call(url,'/management/invoke',sessions['alice'],command)
    assert status==200 and committed['state']=='succeeded' and json.loads(committed['result_json'])['revision']==1
    assert call(url,'/management/invoke',sessions['alice'],command)[:2]==(200,committed)
    bob, pat_cases=prove_personal_token(url,root,deployment,sessions)
    return bob,operation,committed,[
        'actual_password_registration_and_login','distinct_live_account_subjects','cookie_http_only_secure_no_store',
        'wrong_origin_and_password_denied','login_does_not_grant_qualification','missing_and_wrong_csrf_denied',
        'immutable_human_intent','self_approval_denied','distinct_account_approval','single_business_commit']+pat_cases


def prove_personal_token(url,root,deployment,sessions,*,idempotency_key='human-token-1',phase=None):
    expiry=(datetime.now(timezone.utc)+timedelta(minutes=30)).isoformat().replace('+00:00','Z')
    issue={'idempotency_key':idempotency_key,'name':'read-only qualification','deployment':deployment,
        'permissions':['ops.state.read'],'resource_scopes':[{'kind':'ops-state','id':'primary-state'}],'expires_at':expiry}
    assert call(url,'/api/console/v1/human-tokens/issue',sessions['alice'],{**issue,'idempotency_key':'wrong-deployment','deployment':'other-deployment'})[0]==403
    assert call(url,'/api/console/v1/human-tokens/issue',sessions['alice'],{**issue,'idempotency_key':'wrong-scope','resource_scopes':[{'kind':'ops-state','id':'secondary-state'}]})[0]==403
    if phase is not None:
        private(phase/'issue-started.json',json.dumps(issue))
    status,issued,headers=call(url,'/api/console/v1/human-tokens/issue',sessions['alice'],issue)
    assert status==200 and isinstance(issued.get('token'),str) and issued['token'] and not issued['replayed'], 'First PAT issue must return one raw secret'
    token=issued['token'];credential=issued['credential']
    if phase is not None:
        private(phase/'token.secret',token)
        private(phase/'issue-completed.json',json.dumps({'credential':credential,'replayed':issued['replayed']}))
    assert headers.get('Cache-Control')=='no-store'
    assert credential.get('created_at') and not credential.get('last_used_at') and not credential.get('revoked_at'), 'New PAT owner metadata is missing'
    receipt_request={'deployment':deployment,'idempotency_key':issue['idempotency_key']}
    status,receipt,_=call(url,'/api/console/v1/human-tokens/receipt',sessions['alice'],receipt_request)
    assert status==200 and receipt['found'] and receipt['credential']==credential and token not in json.dumps(receipt), 'PAT receipt must reconcile exact metadata only'
    status,replay,_=call(url,'/api/console/v1/human-tokens/issue',sessions['alice'],issue)
    assert status==200 and replay['replayed'] and replay.get('token') is None and replay['credential']==credential, 'PAT replay must not reveal a second secret'
    assert call(url,'/api/console/v1/human-tokens/issue',sessions['alice'],{**issue,'name':'changed'})[0]==409
    status,catalog,_=call(url,'/management-tools/catalog',bearer=token)
    reads=[tool for tool in catalog.get('tools',[]) if tool['name']!='management__status']
    assert status==200 and len(reads)==1 and len(catalog['tools'])==2 and 'approve' not in reads[0]['name'], 'PAT must admit only its current read tool'
    invoke={'name':reads[0]['name'],'arguments_json':'{"input":{}}'}
    status,read,_=call(url,'/management-tools/execute',payload=invoke,bearer=token)
    assert status==200 and json.loads(read['content'])['state']=='succeeded', 'Actual personal token read tool failed'
    status,listing,_=call(url,'/api/console/v1/human-tokens/list',sessions['alice'],{'deployment':deployment,'limit':100})
    active=next(item for item in listing.get('credentials',[]) if item['credential_id']==credential['credential_id'])
    assert status==200 and active.get('last_used_at') and active['created_at']==credential['created_at'] and not active.get('revoked_at'), 'Accepted PAT authentication activity is missing'
    revoke={'deployment':deployment,'credential_id':credential['credential_id']}
    if phase is not None:
        private(phase/'cross-subject-revoke-started.json',json.dumps(revoke))
    denied,_,denied_headers=call(url,'/api/console/v1/human-tokens/revoke',sessions['bob'],revoke)
    if phase is not None:
        private(phase/'cross-subject-revoke-completed.json',json.dumps({'status':denied,
            'no_store':denied_headers.get('Cache-Control')=='no-store'}))
    assert denied in [403,404] and denied_headers.get('Cache-Control')=='no-store'
    if phase is not None:
        private(phase/'revoke-started.json',json.dumps(revoke))
    status,revoked,_=call(url,'/api/console/v1/human-tokens/revoke',sessions['alice'],revoke)
    assert status==200 and revoked['revoked'], 'Human PAT revocation failed'
    if phase is not None:
        private(phase/'revoke-completed.json','{"revoked":true}')
    status,listing,_=call(url,'/api/console/v1/human-tokens/list',sessions['alice'],{'deployment':deployment,'limit':100})
    inactive=next(item for item in listing.get('credentials',[]) if item['credential_id']==credential['credential_id'])
    assert status==200 and not inactive['active'] and inactive.get('revoked_at') and inactive['last_used_at']==active['last_used_at'], 'Token-owner revoke metadata is missing'
    assert call(url,'/management-tools/catalog',bearer=token)[0]==403, 'Revoked personal token retained authority'
    assert call(url,'/management-tools/execute',payload=invoke,bearer=token)[0]==403, 'Previously listed tool retained revoked personal token authority'
    assert call(url,'/auth/logout',sessions['alice'],{})[0]==200
    assert call(url,'/api/console/v1/session',sessions['alice'])[0]==401, 'Logged-out Account session remained valid'
    return sessions['bob'],[
        'one_time_personal_token','exact_metadata_receipt','pat_wrong_deployment_and_scope_denied',
        'pat_replay_has_no_secret','pat_intent_conflict_denied','bounded_pat_agent_read',
        'cross_subject_pat_revoke_denied','fresh_pat_revocation','owner_created_accepted_auth_and_revoked_metadata',
        'account_logout_revokes_session']


def prove_restarted_operation(url,origin,root,operation,committed,bob):
    accounts=json.loads((root/'human-enrollment.json').read_text())['accounts']
    status,session,_=call(url,'/api/console/v1/session',bob)
    assert status==200 and session['subject']==accounts['bob']['subject'], 'The retained decider session did not survive restart'
    status,problem,_=call(url,'/management/operations/'+operation,bob)
    assert (status,problem.get('status'),problem.get('code'))==(403,403,'operators_required'), 'Another subject read the requester operation'
    alice=login(url,origin,'alice@ops.test',(root/'alice-password.secret').read_text())
    status,session,_=call(url,'/api/console/v1/session',alice)
    assert status==200 and session['subject']==accounts['alice']['subject'], 'The requester account did not survive restart'
    assert call(url,'/management/operations/'+operation,alice)[:2]==(200,committed), 'The requester did not recover the exact committed operation'
    status,current,_=call(url,'/management/invoke',bob,{'entry_id':'state.read','version':'2.0.0',
        'input_json':'{}','idempotency_key':None,'expected_revision':None})
    assert status==200 and current['state']=='succeeded', 'The decider lost independent business read access'
    expected=json.loads(committed['result_json']);observed=json.loads(current['result_json'])
    assert (observed['value'],observed['revision'])==(expected['value'],expected['revision']), 'Status recovery changed the business state'
    return ['persistent_decider_account_session','status_is_bound_to_original_requester','status_recovery_does_not_repeat_business_write']


def resume_committed(cli, root, receipt, browser_ready_file):
    assert not receipt.exists() and not (root/'project').exists()
    assert (root/'tool-audience-build-completed.json').exists()
    assert not browser_ready_file.exists() and not browser_ready_file.with_suffix('.done').exists()
    phase=root/'qualification-after-tool-audience';phase.mkdir(mode=0o700)
    with sqlite3.connect('file:'+str(root/'management.sqlite')+'?mode=ro',uri=True) as db:
        rows=db.execute('SELECT operation_id,response_json FROM management_invocations WHERE idempotency_key=?',('human-reviewed-1',)).fetchall()
    assert len(rows)==1
    operation,raw=rows[0];committed=json.loads(raw)
    assert committed['operation_id']==operation and committed['state']=='succeeded'
    assert json.loads(committed['result_json'])['revision']==1
    private(phase/'prior-committed-metadata.json',json.dumps({'operation_id':operation,'receipt':committed}))
    selections=json.loads((root/'operator-input.json').read_text());runtime=json.loads((root/'runtime.json').read_text())
    deployment=selections['deployment'];origin=runtime['origin'];artifact=root/'distribution';facilities=root/'facilities.json'
    enrollment=json.loads((root/'human-enrollment.json').read_text())['accounts']
    evidence={'schema':'lenso.ops-human-qualification.v1','target':'native_http','profile':'password-account-personal-token',
        'real_database':True,'deployment':deployment,'build':native_build(artifact),'cli_sha256':hashlib.sha256(cli.read_bytes()).hexdigest(),
        'source_deleted':True,'human_pat_workers':'unsupported_profile','prior_completed_phase':{
            'build':native_build(root/'distribution-before-tool-audience'),'operation_id':operation,'value':47,'revision':1,
            'enrollment_repeated':False,'business_write_repeated':False}}
    with native_server(cli,artifact,facilities,root) as url:
        assert call(url,'/api/console/v1/session')[0]==401
        assert call(url,'/auth/login',payload={'identifier':'alice@ops.test','password':(root/'alice-password.secret').read_text()},origin='https://wrong.example')[0]==403
        assert call(url,'/auth/login',payload={'identifier':'alice@ops.test','password':'wrong-password'},origin=origin)[0]==403
        sessions={name:login(url,origin,name+'@ops.test',(root/(name+'-password.secret')).read_text()) for name in ['alice','bob']}
        for name,session in sessions.items():
            status,current,headers=call(url,'/api/console/v1/session',session)
            assert status==200 and current['subject']==enrollment[name]['subject'] and headers.get('Cache-Control')=='no-store'
        assert call(url,'/management/operations/'+operation,sessions['alice'])[:2]==(200,committed)
        command={'entry_id':'state.update','version':'2.0.0','input_json':'{"value":47}', 'idempotency_key':'human-reviewed-1','expected_revision':'0'}
        assert call(url,'/management/invoke',sessions['alice'],command,csrf=False)[0]==403
        assert call(url,'/management/invoke',sessions['alice'],command,csrf='mismatch')[0]==403
        assert call(url,'/management/invoke',sessions['alice'],command)[:2]==(200,committed)
        bob,cases=prove_personal_token(url,root,deployment,sessions,idempotency_key='human-token-tool-audience-1',phase=phase)
        evidence['cases']=['actual_password_login','distinct_live_account_subjects','cookie_http_only_secure_no_store',
            'wrong_origin_and_password_denied','reconciled_terminal_human_intent','missing_and_wrong_csrf_denied','single_existing_business_receipt']+cases
    with native_server(cli,artifact,facilities,root) as url:
        evidence['cases']+=prove_restarted_operation(url,origin,root,operation,committed,bob)
        evidence['persistent_account_session_and_operation']='passed'
        pairing={'run_nonce':secrets.token_urlsafe(18),'deployment':deployment,'cli_sha256':evidence['cli_sha256'],
            'host_build_sha256':evidence['build']['receipts']['.lenso/host-build.json']['sha256']}
        private(browser_ready_file,json.dumps({'url':url,'origin':origin,'directory':str(root),**pairing}))
        print(json.dumps({'browser_ready':str(browser_ready_file)}),flush=True)
        deadline=time.monotonic()+3600
        while not browser_ready_file.with_suffix('.done').exists():
            if time.monotonic()>deadline:raise TimeoutError('Actual browser proof did not complete')
            time.sleep(0.2)
        browser=json.loads(browser_ready_file.with_suffix('.done').read_text())
        assert browser['status']=='passed' and browser.get('cases')
        assert all(browser.get(key)==value for key,value in pairing.items()), 'Browser proof belongs to another Host run'
        evidence['browser']=browser
        status,current,_=call(url,'/management/invoke',bob,{'entry_id':'state.read','version':'2.0.0',
            'input_json':'{}','idempotency_key':None,'expected_revision':None})
        assert status==200 and current['state']=='succeeded'
        domain=json.loads(current['result_json']);assert domain['value']==61 and domain['revision']==2
        evidence['final_business_state']={'value':domain['value'],'revision':domain['revision']}
    evidence['shutdown']='passed';receipt.write_text(json.dumps(evidence,indent=2)+'\n')
    private(phase/'completed.json','{"state":"completed"}\n')
    print(json.dumps({'receipt':str(receipt),'cases':evidence['cases']}))


def finish_pat_reconciliation(cli,root,receipt,browser_ready_file):
    phase=root/'qualification-after-tool-audience'
    assert not receipt.exists() and not browser_ready_file.exists()
    assert (phase/'issue-completed.json').exists() and not (phase/'revoke-started.json').exists()
    metadata=json.loads((phase/'issue-completed.json').read_text())['credential']
    token=(phase/'token.secret').read_text();deployment=json.loads((root/'operator-input.json').read_text())['deployment']
    runtime=json.loads((root/'runtime.json').read_text());origin=runtime['origin'];artifact=root/'distribution';facilities=root/'facilities.json'
    prior=json.loads((phase/'prior-committed-metadata.json').read_text());operation=prior['operation_id'];committed=prior['receipt']
    evidence={'schema':'lenso.ops-human-qualification.v1','target':'native_http','profile':'password-account-personal-token',
        'real_database':True,'deployment':deployment,'build':native_build(artifact),'cli_sha256':hashlib.sha256(cli.read_bytes()).hexdigest(),
        'source_deleted':True,'human_pat_workers':'unsupported_profile','prior_completed_phase':{
            'build':native_build(root/'distribution-before-tool-audience'),'operation_id':operation,'value':47,'revision':1,
            'enrollment_repeated':False,'business_write_repeated':False},
        'cases':['actual_password_login','distinct_live_account_subjects','cookie_http_only_secure_no_store',
            'wrong_origin_and_password_denied','reconciled_terminal_human_intent','missing_and_wrong_csrf_denied',
            'single_existing_business_receipt','one_time_personal_token','exact_metadata_receipt',
            'pat_wrong_deployment_and_scope_denied','pat_replay_has_no_secret','pat_intent_conflict_denied','bounded_pat_agent_read']}
    with native_server(cli,artifact,facilities,root) as url:
        sessions={name:login(url,origin,name+'@ops.test',(root/(name+'-password.secret')).read_text()) for name in ['alice','bob']}
        status,catalog,_=call(url,'/management-tools/catalog',bearer=token);assert status==200 and len(catalog['tools'])==2
        read=next(item for item in catalog['tools'] if item['name']!='management__status')
        invoke={'name':read['name'],'arguments_json':'{"input":{}}'}
        status,outcome,_=call(url,'/management-tools/execute',payload=invoke,bearer=token)
        assert status==200 and json.loads(json.loads(outcome['content'])['result_json'])['revision']==1
        listing_request={'deployment':deployment,'limit':100}
        status,listing,_=call(url,'/api/console/v1/human-tokens/list',sessions['alice'],listing_request)
        active=next(item for item in listing['credentials'] if item['credential_id']==metadata['credential_id'])
        assert status==200 and active['active'] and active['last_used_at'] and not active.get('revoked_at')
        revoke={'deployment':deployment,'credential_id':metadata['credential_id']}
        with sqlite3.connect('file:'+str(root/'human-tokens.sqlite')+'?mode=ro',uri=True) as db:
            rows=db.execute('SELECT state_json FROM management_external_mutations WHERE subject=? AND kind=? AND idempotency_key=?',
                (json.loads((root/'human-enrollment.json').read_text())['accounts']['bob']['subject'],
                 'auth.pat.revoke',metadata['credential_id'])).fetchall()
        assert len(rows)==1 and json.loads(rows[0][0])=='failed'
        private(phase/'cached-failed-revoke-reconciliation.json',json.dumps({'prior_denial_vector':'not_qualified',
            'terminal_state':'failed','credential_still_active':True,'issue_repeated':False,'failed_key_replayed':False}))
        private(phase/'revoke-started.json',json.dumps(revoke))
        status,reply,_=call(url,'/api/console/v1/human-tokens/revoke',sessions['alice'],revoke)
        assert status==200 and reply['revoked'];private(phase/'revoke-completed.json','{"revoked":true}')
        status,listing,_=call(url,'/api/console/v1/human-tokens/list',sessions['alice'],listing_request)
        inactive=next(item for item in listing['credentials'] if item['credential_id']==metadata['credential_id'])
        assert status==200 and not inactive['active'] and inactive['revoked_at'] and inactive['last_used_at']==active['last_used_at']
        assert call(url,'/management-tools/catalog',bearer=token)[0]==403
        assert call(url,'/management-tools/execute',payload=invoke,bearer=token)[0]==403
        assert call(url,'/auth/logout',sessions['alice'],{})[0]==200
        assert call(url,'/api/console/v1/session',sessions['alice'])[0]==401
        fresh_phase=phase/'cross-subject-first-reply';fresh_phase.mkdir(mode=0o700)
        sessions['alice']=login(url,origin,'alice@ops.test',(root/'alice-password.secret').read_text())
        bob,fresh_cases=prove_personal_token(url,root,deployment,sessions,
            idempotency_key='human-token-cross-subject-first-reply-2',phase=fresh_phase)
        evidence['cross_subject_revoke']=json.loads((fresh_phase/'cross-subject-revoke-completed.json').read_text())
        evidence['cases']+=['cross_subject_pat_revoke_denied','fresh_pat_revocation',
            'owner_created_accepted_auth_and_revoked_metadata','account_logout_revokes_session']
    with native_server(cli,artifact,facilities,root) as url:
        evidence['cases']+=prove_restarted_operation(url,origin,root,operation,committed,bob)
        evidence['persistent_account_session_and_operation']='passed'
        pairing={'run_nonce':secrets.token_urlsafe(18),'deployment':deployment,'cli_sha256':evidence['cli_sha256'],
            'host_build_sha256':evidence['build']['receipts']['.lenso/host-build.json']['sha256']}
        private(browser_ready_file,json.dumps({'url':url,'origin':origin,'directory':str(root),**pairing}))
        print(json.dumps({'browser_ready':str(browser_ready_file)}),flush=True)
        deadline=time.monotonic()+3600
        while not browser_ready_file.with_suffix('.done').exists():
            if time.monotonic()>deadline:raise TimeoutError('Actual browser proof did not complete')
            time.sleep(0.2)
        browser=json.loads(browser_ready_file.with_suffix('.done').read_text())
        assert browser['status']=='passed' and browser.get('cases') and all(browser.get(key)==value for key,value in pairing.items())
        evidence['browser']=browser
        status,current,_=call(url,'/management/invoke',bob,{'entry_id':'state.read','version':'2.0.0',
            'input_json':'{}','idempotency_key':None,'expected_revision':None})
        assert status==200 and current['state']=='succeeded'
        domain=json.loads(current['result_json']);assert domain['value']==61 and domain['revision']==2
        evidence['final_business_state']={'value':domain['value'],'revision':domain['revision']}
    evidence['shutdown']='passed';receipt.write_text(json.dumps(evidence,indent=2)+'\n')
    private(phase/'completed.json','{"state":"completed"}\n')
    print(json.dumps({'receipt':str(receipt),'state':'passed'}))


def browser_after_known_pat(cli,root,receipt,browser_ready_file):
    phase=root/'qualification-after-tool-audience';fresh=phase/'cross-subject-first-reply'
    assert not receipt.exists() and not browser_ready_file.exists()
    assert (phase/'revoke-completed.json').exists() and (fresh/'revoke-completed.json').exists()
    cross=json.loads((fresh/'cross-subject-revoke-completed.json').read_text())
    assert cross['status']==404 and cross['no_store']
    prior=json.loads((phase/'prior-committed-metadata.json').read_text());operation=prior['operation_id'];committed=prior['receipt']
    runtime=json.loads((root/'runtime.json').read_text());origin=runtime['origin']
    deployment=json.loads((root/'operator-input.json').read_text())['deployment']
    evidence={'schema':'lenso.ops-human-qualification.v1','target':'native_http','profile':'password-account-personal-token',
        'real_database':True,'deployment':deployment,'build':native_build(root/'distribution'),
        'cli_sha256':hashlib.sha256(cli.read_bytes()).hexdigest(),'source_deleted':True,'human_pat_workers':'unsupported_profile',
        'cross_subject_revoke':cross,'prior_completed_phase':{'build':native_build(root/'distribution-before-tool-audience'),
            'operation_id':operation,'value':47,'revision':1,'enrollment_repeated':False,'business_write_repeated':False},
        'cases':['actual_password_login','distinct_live_account_subjects','cookie_http_only_secure_no_store',
            'wrong_origin_and_password_denied','reconciled_terminal_human_intent','missing_and_wrong_csrf_denied',
            'single_existing_business_receipt','one_time_personal_token','exact_metadata_receipt',
            'pat_wrong_deployment_and_scope_denied','pat_replay_has_no_secret','pat_intent_conflict_denied','bounded_pat_agent_read',
            'cross_subject_pat_revoke_denied','fresh_pat_revocation','owner_created_accepted_auth_and_revoked_metadata',
            'account_logout_revokes_session']}
    with native_server(cli,root/'distribution',root/'facilities.json',root) as url:
        sessions={name:login(url,origin,name+'@ops.test',(root/(name+'-password.secret')).read_text()) for name in ['alice','bob']}
        assert call(url,'/management/operations/'+operation,sessions['alice'])[:2]==(200,committed)
        assert call(url,'/management/operations/'+operation,sessions['bob'])[0]==403
        evidence['persistent_account_session_and_operation']='passed'
        evidence['cases'].append('status_is_bound_to_original_requester')
        pairing={'run_nonce':secrets.token_urlsafe(18),'deployment':deployment,'cli_sha256':evidence['cli_sha256'],
            'host_build_sha256':evidence['build']['receipts']['.lenso/host-build.json']['sha256']}
        private(browser_ready_file,json.dumps({'url':url,'origin':origin,'directory':str(root),**pairing}))
        print(json.dumps({'browser_ready':str(browser_ready_file)}),flush=True)
        deadline=time.monotonic()+3600
        while not browser_ready_file.with_suffix('.done').exists():
            if time.monotonic()>deadline:raise TimeoutError('Actual browser proof did not complete')
            time.sleep(0.2)
        browser=json.loads(browser_ready_file.with_suffix('.done').read_text())
        assert browser['status']=='passed' and browser.get('cases') and all(browser.get(key)==value for key,value in pairing.items())
        evidence['browser']=browser
        status,current,_=call(url,'/management/invoke',sessions['bob'],{'entry_id':'state.read','version':'2.0.0',
            'input_json':'{}','idempotency_key':None,'expected_revision':None})
        assert status==200 and current['state']=='succeeded'
        domain=json.loads(current['result_json']);assert domain['value']==61 and domain['revision']==2
        evidence['final_business_state']={'value':domain['value'],'revision':domain['revision']}
    evidence['shutdown']='passed';receipt.write_text(json.dumps(evidence,indent=2)+'\n')
    private(phase/'completed.json','{"state":"completed"}\n')
    print(json.dumps({'receipt':str(receipt),'state':'passed'}))


def run():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli',type=Path,required=True);parser.add_argument('--database-url-file',type=Path,required=True)
    parser.add_argument('--audit-url-file',type=Path,required=True);parser.add_argument('--receipt',type=Path,required=True)
    parser.add_argument('--directory',type=Path);parser.add_argument('--public-origin')
    parser.add_argument('--resume-build',action='store_true',help='Resume failed initial compilation before enrollment started')
    parser.add_argument('--browser-ready-file',type=Path,help='Hold the real Host for the browser proof until the paired .done file exists')
    args=parser.parse_args();fixture=Path(__file__).parent.resolve()
    if args.browser_ready_file and args.browser_ready_file.with_suffix('.done').exists():
        parser.error('A prior browser completion file cannot qualify a new Host run')
    root=args.directory.resolve() if args.directory else Path(tempfile.mkdtemp(prefix='lenso-ops-human-'))
    root.mkdir(exist_ok=True);source=root/'project'
    if args.resume_build:
        if not args.directory or (root/'enrollment-started.json').exists():parser.error('Enrollment may not be blindly repeated')
        input=json.loads((root/'operator-input.json').read_text());runtime=json.loads((root/'runtime.json').read_text())
        native_port=runtime['native_port'];origin=runtime['origin']
    else:
        shutil.copytree(fixture/'project',source,ignore=shutil.ignore_patterns('.lenso','target','dist'))
        shutil.copytree(fixture/'storage',root/'storage',ignore=shutil.ignore_patterns('target'))
        for name,file in [('database-uri',args.database_url_file),('audit-uri',args.audit_url_file)]:private(root/(name+'.secret'),file.read_text().strip())
        for name in ['signing','pepper','account-signing','account-pepper','alice-password','bob-password']:private(root/(name+'.secret'),secrets.token_urlsafe(36))
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1',0));native_port=reservation.getsockname()[1]
        origin=args.public_origin or f'http://localhost:{native_port}'
        prefix='ops_human_'+str(secrets.randbelow(1_000_000_000))
        input={'database_uri_file':str(root/'database-uri.secret'),'audit_uri_file':str(root/'audit-uri.secret'),
            'directory':str(root),'schemas':{name:prefix+'_'+name for name in ['auth','access','approval']},
            'deployment':prefix,'issuer':prefix+'.operators','signing_file':str(root/'signing.secret'),
            'pepper_file':str(root/'pepper.secret'),'resource_uri':f'http://localhost:{native_port}/mcp'}
        private(root/'operator-input.json',json.dumps(input));private(root/'runtime.json',json.dumps({'native_port':native_port,'origin':origin}))
    result=subprocess.check_output(['cargo','build','--locked','--manifest-path',str(fixture/'operator/Cargo.toml'),'--message-format=json-render-diagnostics'],text=True)
    records=[json.loads(line) for line in result.splitlines() if line.startswith('{')]
    executable=next(Path(item['executable']) for item in records if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name']=='lenso-ops-reference-operator')
    operator=root/'operator';shutil.copy2(executable,operator)
    if not args.resume_build:
        for action in ['setup','human-setup']:subprocess.run([str(operator),action,str(root/'operator-input.json')],check=True)
        uri=(root/'database-uri.secret').read_text()
        for instance,value in [('primary',11),('secondary',29)]:
            schema=input['schemas']['auth']+'_'+instance
            sql(uri,f'BEGIN;CREATE SCHEMA {schema};SET search_path TO {schema};\n'+(fixture/'storage/native-pg/schema.sql').read_text()+f"\nINSERT INTO state VALUES(true,'{instance}-state',{value},0);COMMIT;")
    setup=json.loads((root/'setup.json').read_text());human=json.loads((root/'human-setup.json').read_text())
    facilities=profile(source,root,input,setup,human,native_port,origin)
    build=lambda out:subprocess.run([str(args.cli),'app','build','--root',str(source),'--out',str(out)],check=True)
    artifact=root/'enrollment-distribution';build(artifact)
    (root/'enrollment-started.json').write_text(json.dumps({'deployment':input['deployment']}))
    with native_server(args.cli,artifact,facilities,root,ready_marker='Explicit human bootstrap completed'):pass
    (source/'plugins/example.ops-human-bootstrap/default.toml').unlink();(source/'plugins/example.ops-human-bootstrap/default.disabled').touch()
    grants=json.loads(facilities.read_text());del grants['instances']['example.ops-human-bootstrap/default'];private(facilities,json.dumps(grants))
    artifact=root/'distribution';build(artifact)
    evidence={'schema':'lenso.ops-human-qualification.v1','target':'native_http','profile':'password-account-personal-token',
        'real_database':True,'deployment':input['deployment'],'build':native_build(artifact),'cli_sha256':hashlib.sha256(args.cli.read_bytes()).hexdigest(),
        'source_deleted':True,'human_pat_workers':'unsupported_profile'}
    shutil.rmtree(source);shutil.rmtree(root/'storage',ignore_errors=True)
    with native_server(args.cli,artifact,facilities,root) as url:
        bob,operation,committed,evidence['cases']=prove(url,origin,root,input['deployment'])
    with native_server(args.cli,artifact,facilities,root) as url:
        evidence['cases']+=prove_restarted_operation(url,origin,root,operation,committed,bob)
        evidence['persistent_account_session_and_operation']='passed'
        if args.browser_ready_file:
            pairing={'run_nonce':secrets.token_urlsafe(18),'deployment':input['deployment'],
                'cli_sha256':evidence['cli_sha256'],
                'host_build_sha256':evidence['build']['receipts']['.lenso/host-build.json']['sha256']}
            private(args.browser_ready_file,json.dumps({'url':url,'origin':origin,'directory':str(root),**pairing}))
            print(json.dumps({'browser_ready':str(args.browser_ready_file)}),flush=True)
            deadline=time.monotonic()+1800
            while not args.browser_ready_file.with_suffix('.done').exists():
                if time.monotonic()>deadline:raise TimeoutError('Actual browser proof did not complete')
                time.sleep(0.2)
            browser=json.loads(args.browser_ready_file.with_suffix('.done').read_text())
            assert browser['status']=='passed' and browser.get('cases'), 'Actual browser cases did not pass'
            assert all(browser.get(key)==value for key,value in pairing.items()), 'Browser proof belongs to another Host run'
            evidence['browser']=browser
        status,current,_=call(url,'/management/invoke',bob,{'entry_id':'state.read','version':'2.0.0',
            'input_json':'{}','idempotency_key':None,'expected_revision':None})
        assert status==200 and current['state']=='succeeded', 'Final business state unavailable'
        domain=json.loads(current['result_json'])
        evidence['final_business_state']={'value':domain['value'],'revision':domain['revision']}
    evidence['shutdown']='passed';args.receipt.parent.mkdir(parents=True,exist_ok=True)
    args.receipt.write_text(json.dumps(evidence,indent=2)+'\n');print(json.dumps({'receipt':str(args.receipt),'cases':evidence['cases']}))


if __name__=='__main__':run()
