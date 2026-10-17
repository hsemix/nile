import native from '@native';
// Non-database event source; background notifications use the same public SDK.
export function subscribe(listener) {
  const subscription = native.subscribe('notifications.watch', {}, packet => listener(packet.value));
  return { ready: subscription.ready.then(packet => packet.value), unsubscribe: subscription.unsubscribe };
}
export default Object.freeze({ subscribe });
