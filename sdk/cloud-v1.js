// Experimental cloud checkpoint wire format v1. Overall SDK 1.0 is not yet frozen.
export class CloudSaveError extends Error {
  constructor(message, code, status) { super(message); this.name='CloudSaveError'; this.code=code; this.status=status; }
}
const path = group => '/_infuser/cloud/v1/groups/' + encodeURIComponent(group);
async function request(url, body) {
  let response;
  try {
    response=await fetch(url,{credentials:'same-origin',signal:AbortSignal.timeout(10000),...(body===undefined?{}:{method:'POST',headers:{'Content-Type':'application/json'},body})});
  } catch {
    throw new CloudSaveError('Could not confirm the save. Keep your pending checkpoint and retry with the same commit ID.','network_error',0);
  }
  let result;
  try { result=await response.json(); }
  catch { throw new CloudSaveError('The save response was interrupted. Retry with the same commit ID.','invalid_response',response.ok?502:response.status); }
  if(!result || typeof result!=='object' || Array.isArray(result))throw new CloudSaveError('Invalid cloud save response.','invalid_response',502);
  if(response.ok || (response.status===409 && result.status==='conflict'))return result;
  throw new CloudSaveError(result.error?.message || (typeof result.error==='string'?result.error:'Cloud save request failed.'),result.error?.code||'http_error',response.status);
}
async function snapshot(url) {
  const value=await request(url);
  if(!(value.revision===null || typeof value.revision==='string') || !value.files || typeof value.files!=='object' || Array.isArray(value.files))throw new CloudSaveError('The server did not return a complete checkpoint.','invalid_response',502);
  return value;
}
export const cloud = {
  // Explicit conversion: JSON values are never guessed to be binary.
  encodeBinary(bytes) {
    if (!(bytes instanceof Uint8Array)) throw new CloudSaveError('Pass a Uint8Array to encodeBinary.','invalid_binary_file',400);
    let text='';
    for(let offset=0;offset<bytes.length;offset+=8192)text+=String.fromCharCode(...bytes.subarray(offset,offset+8192));
    return {base64:btoa(text)};
  },
  decodeBinary(file) {
    try {
      if(!file || Object.keys(file).length!==1 || typeof file.base64!=='string')throw Error();
      const text=atob(file.base64);
      if(btoa(text)!==file.base64)throw Error();
      return Uint8Array.from(text,c=>c.charCodeAt(0));
    } catch { throw new CloudSaveError('Invalid binary save file. Keep the existing local save.','invalid_binary_file',400); }
  },
  load: group => snapshot(path(group)),
  read: (group,revision) => snapshot(path(group)+'/revisions/'+encodeURIComponent(revision)),
  history: group => request(path(group)+'/history'),
  async commit(group,{expectedRevision,commitId,files,summary='',resolveCandidate=null}) {
    if(expectedRevision===undefined || !commitId)throw new CloudSaveError('Supply expectedRevision (null for a new group) and a unique commitId.','invalid_checkpoint',400);
    // Serialize once: mutations while awaiting a retry cannot change this write.
    const body=JSON.stringify({expected_revision:expectedRevision,commit_id:commitId,files,summary,resolve_candidate:resolveCandidate});
    for(let attempt=0;;attempt++) {
      try {
        const result=await request(path(group),body);
        if((result.status==='saved' && typeof result.revision==='string') || (result.status==='conflict' && typeof result.current_revision==='string' && typeof result.candidate_revision==='string'))return result;
        throw new CloudSaveError('The server did not confirm this checkpoint.','invalid_response',502);
      }
      catch(error) {
        error.commitId=commitId;
        if(attempt>=2 || !(error.status===0 || [500,502,503,504].includes(error.status)))throw error;
        await new Promise(resolve=>setTimeout(resolve,250*(attempt+1)));
      }
    }
  },
  resolve(group,{candidateRevision,...checkpoint}) {
    if(!candidateRevision)throw new CloudSaveError('Choose a preserved conflict candidate.','invalid_checkpoint',400);
    return cloud.commit(group,{...checkpoint,resolveCandidate:candidateRevision});
  }
};
