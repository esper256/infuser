// The selector is tab-local, not a credential. Every request is authorized by
// the server against the authenticated account. Keep this page's selector pinned.
const KEY='infuser.profile';
let selected;
try { selected=sessionStorage.getItem(KEY)||null; } catch { selected=null; }
export function nextPage(value=location.pathname+location.search+location.hash){
  const url=new URL(value,location.origin);
  return url.origin===location.origin&&/^\/(?:library|clubhouse|desktop|games\/[a-z][a-z0-9-]{0,23})$/.test(url.pathname)
    ?url.pathname+url.search+url.hash:'/library';
}
export function forgetProfile(){
  for(const key of [KEY,'infuser.visit','infuser.group-inspection']){
    try{sessionStorage.removeItem(key);}catch{/* The current page stays pinned until navigation. */}
  }
}
export function pinProfile(id,requireStorage=false){
  try{sessionStorage.setItem(KEY,id);}catch{if(requireStorage)throw Error('Allow site storage to switch profiles.');}
  selected=id;
}
export function chooser(next=nextPage()){
  forgetProfile();location.assign('/profiles?next='+encodeURIComponent(nextPage(next)));
}
export async function profileFetch(path,options={}){
  const headers=new Headers(options.headers);
  if(selected)headers.set('X-Infuser-Profile',selected);
  const response=await fetch(path,{...options,headers});
  if(response.headers.get('x-infuser-profile-required')==='1'){
    chooser();const error=Error('Choose your profile to continue.');error.profileRequired=true;throw error;
  }
  return response;
}
