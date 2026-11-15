// A real workerd stand-in, never a product CentralObject or admission oracle.
export const attachmentWitnessHeader = "x-hibernation-attachment";
export const hibernationWorkerSource = `
export class HibernationWitness {
  constructor(state, env) {
    this.state = state;
    this.env = env;
    state.blockConcurrencyWhile(async () => {
      const response = await env.WITNESS.fetch('https://witness/record', {
        method: 'POST', body: JSON.stringify({kind: 'constructor'})
      });
      this.instance = await response.text();
      // A singleton test counter witnesses storage continuity, not a domain entity:
      // no generated row identity or creation/update timestamps are meaningful here.
      state.storage.sql.exec('CREATE TABLE IF NOT EXISTS frame_count (count INTEGER)');
      state.storage.sql.exec('INSERT INTO frame_count SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM frame_count)');
    });
  }
  async fetch(request) {
    if (new URL(request.url).pathname !== '/socket') return new Response('unknown', {status: 404});
    const [client, server] = Object.values(new WebSocketPair());
    // Only this upgrade sees the per-connection witness; after fetch returns,
    // a fresh instance can recover it only from the socket attachment.
    const witness = request.headers.get('${attachmentWitnessHeader}');
    if (witness === null) return new Response('missing attachment witness', {status: 400});
    const attachment = {witness};
    if (this.env.HIBERNATING === 'true') {
      this.state.acceptWebSocket(server);
      server.serializeAttachment(attachment);
      this.state.setWebSocketAutoResponse(new WebSocketRequestResponsePair('ping', 'pong'));
    } else {
      server.accept();
      server.addEventListener('message', event => this.reply(server, event.data, attachment));
    }
    server.send(JSON.stringify({kind: 'welcome', instance: this.instance, attachment}));
    return new Response(null, {status: 101, webSocket: client});
  }
  async webSocketMessage(socket, message) {
    await this.reply(socket, message, socket.deserializeAttachment());
  }
  async reply(socket, message, attachment) {
    await this.env.WITNESS.fetch('https://witness/record', {
      method: 'POST', body: JSON.stringify({kind: 'handler'})
    });
    if (message === 'ping') { socket.send('pong'); return; }
    if (message !== 'observe') { socket.close(1008, 'unknown fixture frame'); return; }
    this.state.storage.sql.exec('UPDATE frame_count SET count = count + 1');
    const count = this.state.storage.sql.exec('SELECT count FROM frame_count').one().count;
    socket.send(JSON.stringify({kind: 'observed', instance: this.instance, attachment, count}));
  }
  webSocketClose(socket, code, reason) { socket.close(code, reason); }
}
export default {
  fetch(request, env) {
    const id = env.TARGET.idFromName(env.OBJECT_NAME);
    return env.TARGET.get(id).fetch(request);
  }
};
`;
