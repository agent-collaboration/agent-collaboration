import assert from "node:assert/strict";

/** Decodes the POSIX shell words emitted by the fixed source command carrier. */
export function sourceShellWords(command: string): string[] {
  const words: string[] = [];
  let word = "";
  let started = false;
  let quote: "'" | '"' | undefined;
  for (let index = 0; index < command.length; index += 1) {
    const character = command[index];
    assert.ok(character !== undefined);
    if (quote === "'") {
      if (character === "'") quote = undefined;
      else word += character;
    } else if (character === "\\") {
      const next = command[index + 1];
      assert.ok(next !== undefined, "missing escaped character");
      if (quote === '"' && next !== '"' && next !== "\\") word += "\\";
      word += next;
      index += 1;
      started = true;
    } else if (quote === '"') {
      if (character === '"') quote = undefined;
      else word += character;
    } else if (character === "'" || character === '"') {
      quote = character;
      started = true;
    } else if (" \t\r\n".includes(character)) {
      if (started) words.push(word);
      word = "";
      started = false;
    } else {
      word += character;
      started = true;
    }
  }
  assert.equal(quote, undefined, "unclosed shell quote");
  if (started) words.push(word);
  return words;
}
