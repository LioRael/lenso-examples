const canonicalDataPlaneOperations = [
  "direct_request",
  "event",
  "durable_workflow",
  "inbox",
  "outbox",
  "timer",
  "retry",
  "compensation",
  "runtime_story",
];

export function canonicalDataPlaneOperationResults(operationResults) {
  const actual = Object.keys(operationResults).sort();
  const expected = [...canonicalDataPlaneOperations].sort();
  if (
    actual.length !== expected.length
    || actual.some((operation, index) => operation !== expected[index])
  ) {
    throw new Error("outage observation has an unexpected Data Plane operation set");
  }

  return Object.fromEntries(
    canonicalDataPlaneOperations.map((operation) => {
      const result = operationResults[operation];
      if (typeof result !== "boolean") {
        throw new Error(`outage observation operation is not boolean: ${operation}`);
      }
      return [operation, result];
    }),
  );
}
