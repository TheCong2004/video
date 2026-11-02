import test from 'node:test';
import assert from 'node:assert/strict';
import {Readable, PassThrough} from 'node:stream';
import {synthesize} from '../tts-service.mjs';
const voice={voice:'vi-VN-NamMinhNeural',pitch:-18,rate:-20};

test('complete audio is returned and the client closes',async()=>{
  let closes=0;
  const bytes=await synthesize('Cầu an',voice,{createClient:()=>({setMetadata:async()=>{},toStream:()=>({audioStream:Readable.from([Buffer.from('complete-audio')])}),close:()=>closes++})});
  assert.equal(bytes.toString(),'complete-audio');assert.equal(closes,1);
});
test('ECONNRESET retries once and closes both clients',async()=>{
  let attempts=0,closes=0;
  const bytes=await synthesize('Cầu an',voice,{createClient:()=>{const n=++attempts;return{setMetadata:async()=>{if(n===1)throw Object.assign(new Error('reset'),{code:'ECONNRESET'});},toStream:()=>({audioStream:Readable.from([Buffer.from('audio')])}),close:()=>closes++};}});
  assert.equal(bytes.toString(),'audio');assert.equal(attempts,2);assert.equal(closes,2);
});
test('a stalled provider is bounded to two attempts',async()=>{
  let attempts=0,closes=0;
  await assert.rejects(synthesize('Cầu an',voice,{timeoutMs:20,createClient:()=>{attempts++;return{setMetadata:()=>new Promise(()=>{}),close:()=>closes++};}}),{code:'TTS_TIMEOUT_OR_CLOSED'});
  assert.equal(attempts,2);assert.equal(closes,2);
});
test('stop cancels immediately and does not retry',async()=>{
  let attempts=0,closes=0;const abort=new AbortController();
  const promise=synthesize('Cầu an',voice,{signal:abort.signal,timeoutMs:1000,createClient:()=>{attempts++;return{setMetadata:()=>new Promise(()=>{}),close:()=>closes++};}});
  abort.abort();await assert.rejects(promise,{code:'TTS_CANCELLED'});
  assert.equal(attempts,1);assert.equal(closes,1);
});
test('a truncated stream is never published as complete audio',async()=>{
  await assert.rejects(synthesize('Cầu an',voice,{maxAttempts:1,createClient:()=>({setMetadata:async()=>{},toStream:()=>{const audioStream=new PassThrough();queueMicrotask(()=>{audioStream.write('partial');audioStream.destroy();});return{audioStream};},close:()=>{}})}),{code:'TTS_TIMEOUT_OR_CLOSED'});
});
