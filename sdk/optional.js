// Copy/bundle this helper with the game. Standalone launches make no Infuser requests.
// The publisher adds the SDK script to the exported page, not the source project.
export async function connectInfuser() {
  const sdkPath = '/_infuser/sdk/v1.js';
  if (![...document.scripts].some(script => script.getAttribute('src') === sdkPath)) return null;
  const sdk = await import(/* @vite-ignore */ sdkPath);
  // A hosted launch with a failed/expired session must fail, never silently switch
  // to public matchmaking or the owner's standalone saves.
  await sdk.ready;
  return sdk.Infuser;
}
