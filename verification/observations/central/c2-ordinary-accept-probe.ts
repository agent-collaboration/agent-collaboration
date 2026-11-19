// Lead-authorized ONE feasibility probe. Not yet a qualified negative control or
// a selected product mechanism. Failure ends the probe; no alternate runtime path.
import assert from "node:assert/strict";
import { c2WitnessWrapperSource } from "./c2-witness.ts";

export function ordinaryAcceptProbeSource(): string {
  const constructor = "super(state, env);";
  const attachments = "this.witnessState.getWebSockets()";
  assert.equal(
    c2WitnessWrapperSource.split(constructor).length,
    2,
    "one compiled constructor delegation",
  );
  assert.equal(
    c2WitnessWrapperSource.split(attachments).length,
    2,
    "one initial attachment observation",
  );
  return c2WitnessWrapperSource
    .replace(
      constructor,
      `
    const ordinarySockets = [];
    let serving;
    const ordinaryState = new Proxy(state, {get(target, property) {
      if (property === 'acceptWebSocket') return socket => {
        socket.accept();
        ordinarySockets.push(socket);
        socket.addEventListener('message', event => {
          Promise.resolve(serving.webSocketMessage(socket, event.data)).catch(() => {
            socket.close(1011, 'ordinary probe handler failed');
          });
        });
      };
      const value = Reflect.get(target, property, target);
      return typeof value === 'function' ? value.bind(target) : value;
    }});
    super(ordinaryState, env);
    serving = this;
    this.ordinaryProbeSockets = ordinarySockets;
  `,
    )
    .replace(attachments, "this.ordinaryProbeSockets");
}
