import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const source=readFileSync(new URL('../web/voice.js',import.meta.url),'utf8').replaceAll('export function ','function ');
function boot({blocked=false}={}){
  let now=0;const notes=[],contexts=[],timers=new Map();let serial=0;
  class AudioContext {
    constructor(){if(blocked)throw Error('audio unavailable');this.state='running';this.currentTime=0;this.destination={};contexts.push(this);}
    resume(){return Promise.resolve();}
    close(){this.state='closed';return Promise.resolve();}
    createOscillator(){const oscillator={frequency:{value:0},connect(){},disconnect(){},start(){notes.push(oscillator.frequency.value);},stop(){}};return oscillator;}
    createGain(){return {gain:{setValueAtTime(){},linearRampToValueAtTime(){},exponentialRampToValueAtTime(){}},connect(target){assert.equal(target,contexts.at(-1).destination,'tones go only to local output');},disconnect(){}};}
  }
  const create=vm.runInNewContext(source+';createVoiceChimes',{AudioContext,performance:{now:()=>now},setTimeout:fn=>{timers.set(++serial,fn);return serial;},clearTimeout:id=>timers.delete(id)});
  return {chimes:create(),notes,contexts,tick:ms=>{now+=ms;},flush:()=>{for(const fn of timers.values())fn();timers.clear();}};
}
test('join rises, leave falls, and bursts share a cooldown without a backlog',()=>{
  const b=boot();b.chimes.play(true);assert.equal(b.notes.length,0,'silent before opting into voice');
  b.chimes.unlock();b.chimes.play(true);
  assert(b.notes[0]<b.notes[1]&&b.notes[1]<b.notes[2]);
  for(let i=0;i<100;i++)b.chimes.play(i%2===0);
  b.tick(3999);b.chimes.play(false);assert.equal(b.notes.length,3);
  b.tick(1);b.flush();assert.equal(b.notes.length,3,'cooldown never queues events');
  b.chimes.play(false);assert.equal(b.notes.length,6);
  assert(b.notes[3]>b.notes[4]&&b.notes[4]>b.notes[5]);
});
test('reconnects and suspended audio discard stale notifications',()=>{
  const b=boot();b.chimes.unlock();b.chimes.quiet();b.chimes.play(true);assert.equal(b.notes.length,0);
  b.tick(2000);b.chimes.play(true);assert.equal(b.notes.length,3);
  b.tick(5000);b.contexts[0].state='suspended';b.chimes.play(false);assert.equal(b.notes.length,3);
  b.contexts[0].state='running';b.flush();assert.equal(b.notes.length,3);
});
test('off closes audio, quick rejoin cancels close without bypassing cooldown',()=>{
  const b=boot();b.chimes.unlock();b.chimes.play(true);b.chimes.close();b.chimes.unlock();b.flush();
  assert.equal(b.contexts[0].state,'running');b.chimes.play(true);assert.equal(b.notes.length,3);
  b.chimes.close();b.flush();assert.equal(b.contexts[0].state,'closed');
  b.chimes.unlock();b.chimes.play(true);assert.equal(b.notes.length,3);
  b.chimes.close(true);assert.equal(b.contexts[1].state,'closed');
});
test('unavailable audio does not throw or prevent using voice',()=>{
  const b=boot({blocked:true});b.chimes.unlock();b.chimes.play(true);b.chimes.close(true);assert.equal(b.notes.length,0);
});
