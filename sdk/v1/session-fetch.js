export const sessionDenials = new EventTarget();

export async function sessionFetch(url, options) {
  const response = await fetch(url, options);
  if ([401, 403, 410].includes(response.status)) {
    sessionDenials.dispatchEvent(new CustomEvent('denied', {detail: response.status}));
  }
  return response;
}
