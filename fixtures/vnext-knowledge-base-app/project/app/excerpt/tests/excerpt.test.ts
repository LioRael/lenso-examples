import { expect, test } from "bun:test";
import { excerpt } from "../src/excerpt.ts";

test("excerpt truncation keeps combining marks and joined emoji intact", () => {
  expect(excerpt("  e\u0301  👩‍💻  x  ", 4)).toBe("e\u0301 👩‍💻…");
});
