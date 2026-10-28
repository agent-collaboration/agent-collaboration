import { once } from "node:events";
import { createConnection } from "node:net";

export async function sendSocketNotice(path: string, bytes: string): Promise<void> {
  const notice = createConnection(path);
  try {
    await once(notice, "connect");
    await new Promise<void>((resolve, reject): void => {
      notice.once("error", reject);
      notice.end(bytes, resolve);
    });
  } finally {
    notice.destroy();
  }
}
