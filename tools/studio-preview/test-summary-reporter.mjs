// Compact local regression output: retain failures, avoid printing entire source files.
export default async function* summarize(events) {
  for await (const event of events) {
    if (event.type === 'test:fail') {
      const error = event.data.details.error;
      yield `FAIL ${event.data.name}\n${String(error?.message ?? '').slice(0, 230)}\n${String(error?.stack ?? '').split('\n').filter(line => line.includes('.test.mjs:')).join('\n')}\n`;
    }
    if (event.type === 'test:summary') yield `${JSON.stringify(event.data)}\n`;
  }
}
