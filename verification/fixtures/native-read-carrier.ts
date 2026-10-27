import { AcpStdio } from "./acp-stdio.ts";

const io = new AcpStdio();
try {
  for (;;) {
    const request = await io.readRequest();
    if (request === undefined) break;
    if (request.id === 1n) io.send({ id: 1n, result: {} });
    else if (request.id === 2n)
      io.send({
        id: 2n,
        result: {
          thread: {
            id: "fixture-thread",
            turns: [
              {
                id: "fixture-turn",
                items: [
                  {
                    type: "userMessage",
                    id: "fixture-native-item",
                    clientId: "fixture-push",
                    content: [{ type: "text", text: "fixture" }],
                  },
                ],
              },
            ],
          },
        },
      });
  }
} finally {
  io.close();
}
