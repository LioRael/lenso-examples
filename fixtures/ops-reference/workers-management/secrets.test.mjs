import assert from 'node:assert/strict';
import test from 'node:test';
import {create} from './project/app/management/secrets/src/secrets.mjs';

test('selected Worker secret references require exact caller and live event scope', async () => {
  const signing='s'.repeat(32), pepper='p'.repeat(32);
  let active=true, calls=0;
  const scope={run:async work=>{if(!active)throw new Error('event_closed');calls+=1;return work();}};
  const configuration={caller_instance:'lenso.auth.api-token/default',signing_reference:'operators/api-signing',pepper_reference:'operators/api-pepper'};
  const selected=create(JSON.stringify({'operators/api-signing':signing,'operators/api-pepper':pepper}),scope,configuration);
  assert.equal(await selected.resolve('operators/api-signing','lenso.auth.api-token/default'),signing);
  assert.equal(await selected.resolve('operators/api-signing','example.ops-management/default'),null);
  assert.equal(await selected.resolve('operators/database','lenso.auth.api-token/default'),null);
  assert.equal(calls,3);
  assert.throws(()=>create(JSON.stringify({'operators/api-signing':signing,'operators/api-pepper':pepper,extra:'x'}),scope,configuration));
  active=false;
  await assert.rejects(selected.resolve('operators/api-pepper','lenso.auth.api-token/default'),/event_closed/);
});
