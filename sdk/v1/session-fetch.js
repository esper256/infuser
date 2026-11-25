export const sessionDenials = new EventTarget();

export async function sessionFetch(url, options) {
  const response = await fetch(url, options);
  if (response.headers.get('Infuser-Session') === 'ended') {
    sessionDenials.dispatchEvent(new CustomEvent('denied', {detail: response.status}));
  }
  return response;
}
