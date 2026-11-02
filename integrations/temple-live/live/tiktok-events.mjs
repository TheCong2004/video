// Normalize the actual modern protobuf events and the older connector shape.
// No account/session data or invented participant IDs are introduced here.
function text(value){return value==null?'':String(value);}
function firstUrl(value){return Array.isArray(value)?text(value[0]):typeof value==='string'?value:'';}
export function liveUser(user={}){
  let id=text(user.idStr||user.id||user.userId||user.uniqueId||user.displayId);
  if(id==='[object Object]')id='';
  return {id:id.slice(0,128),name:text(user.nickname||user.displayId||user.uniqueId||id),
    avatar:firstUrl(user.profilePicture?.url)||firstUrl(user.profilePicture?.urlList)||firstUrl(user.avatarThumb?.urlList)||firstUrl(user.avatarThumb?.url)};
}
export function liveChat(data){return {...liveUser(data.user||data),text:text(data.content??data.comment)};}
export function liveGift(data){
  const gift=data.giftDetails||data.gift||{},extra=data.extendedGiftInfo||{};
  const type=Number(gift.giftType??gift.type??extra.type??data.giftType);
  const end=data.repeatEnd??data.repeatend??data.isRepeatEnd;
  if(type===1 && !(end===true||end===1||end==='1'))return null;
  const raw=gift.diamondCount??gift.diamond_count??extra.diamond_count??data.diamondCount??data.diamond_count;
  const diamonds=raw==null?NaN:Number(raw),known=Number.isFinite(diamonds)&&diamonds>=0;
  return {...liveUser(data.user||data),giftName:text(gift.giftName||gift.name||extra.name||data.giftName||'quà'),
    count:Math.max(1,Math.min(9999,Number(data.repeatCount??data.repeat_count??data.count)||1)),
    diamonds:known?Math.min(50000,diamonds):0,valueKnown:known};
}
