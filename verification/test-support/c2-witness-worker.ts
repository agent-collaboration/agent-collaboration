// A loader/wrapper qualification stand-in. Never a CentralObject admission proof.
export const c2WitnessQualificationSource = `
export class CentralObject {
  constructor(state, env) { this.state = state; this.env = env; }
  async fetch(request) {
    const [client, server] = Object.values(new WebSocketPair());
    this.state.acceptWebSocket(server);
    server.serializeAttachment(JSON.parse(this.env.ATTACHMENT));
    this.state.setWebSocketAutoResponse(new WebSocketRequestResponsePair('ping','pong'));
    server.send('welcome');
    return new Response(null, {status:101, webSocket:client});
  }
  async webSocketMessage(socket, message) {
    const source = await this.env.TEST_SIGNIN_CLOCK.fetch(
      'https://signin-clock.test/clock/' + socket.deserializeAttachment().organizationId);
    socket.send(JSON.stringify({delegated:message, clock:await source.json()}));
  }
  webSocketClose(socket, code, reason) { socket.close(code, reason); }
}
export default { fetch(request, env) {
  const organization = new URL(request.url).pathname.slice(1);
  return env.CENTRAL.get(env.CENTRAL.idFromName(organization)).fetch(request);
} };
`;
