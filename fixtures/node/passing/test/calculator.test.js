import { test } from "node:test";
import assert from "node:assert/strict";

const add = (a, b) => a + b;
const multiply = (a, b) => a * b;

test("adds two numbers", () => assert.equal(add(2, 3), 5));
test("multiplies two numbers", () => assert.equal(multiply(2, 3), 6));
test("skipped on purpose", { skip: "seeded skip: proves skipped tests are reported" }, () => {
  assert.fail("never runs");
});
