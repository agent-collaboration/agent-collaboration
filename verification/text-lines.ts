const lineBreaks = new RegExp(
  `[${String.fromCodePoint(10, 11, 12, 13, 28, 29, 30, 133, 8232, 8233)}]`,
);

export function splitTextLines(text: string): string[] {
  return text.replaceAll("\r\n", "\n").split(lineBreaks);
}
