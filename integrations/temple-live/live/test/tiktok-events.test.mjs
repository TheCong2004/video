import test from 'node:test';
import assert from 'node:assert/strict';
import {liveChat,liveGift,liveUser} from '../tiktok-events.mjs';
test('modern observed protobuf chat uses content and user id/displayId',()=>{
 const e=liveChat({content:'Nam mô A Di Đà Phật',user:{id:'725000000000000001',displayId:'qa_dao_huu',nickname:'Đạo hữu',avatarThumb:{urlList:['https://example.test/avatar.jpg']}}});
 assert.equal(e.id,'725000000000000001');assert.equal(e.text,'Nam mô A Di Đà Phật');assert.equal(e.name,'Đạo hữu');assert.equal(e.avatar,'https://example.test/avatar.jpg');
 assert.equal(liveChat({comment:'Cầu an',user:{uniqueId:'legacy'}}).text,'Cầu an');
 assert.equal(liveUser({}).id,'');
});
test('gift streak is counted only on final event and keeps actual message value',()=>{
 const message={user:{id:'123',nickname:'QA'},giftDetails:{type:1,name:'Hoa sen',diamondCount:30},repeatCount:3,repeatEnd:0};
 assert.equal(liveGift(message),null);
 const e=liveGift({...message,repeatEnd:1});assert.equal(e.giftName,'Hoa sen');assert.equal(e.count*e.diamonds,90);assert.equal(e.valueKnown,true);
 const unknown=liveGift({user:{id:'123'},giftDetails:{name:'Quà chưa có giá'}});assert.equal(unknown.valueKnown,false);assert.equal(unknown.diamonds,0);
});
