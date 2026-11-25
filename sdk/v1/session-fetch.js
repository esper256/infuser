export const sessionDenials = new EventTarget();

export async function sessionFetch(url, options) {
  const response = await fetch(url, options);
  if (response.headers.get('Infuser-Session') === 'ended') {
    const body = await response.clone().json().catch(() => null);
    const detail = {status: response.status, error: body?.error};
    sessionDenials.dispatchEvent(new CustomEvent('denied', {detail}));
  }
  return response;
}
