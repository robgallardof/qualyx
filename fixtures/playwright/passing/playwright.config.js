// @ts-check
const { defineConfig } = require("@playwright/test");

module.exports = defineConfig({
  testDir: "tests",
  retries: 0,
  workers: 1,
  use: { browserName: "chromium", headless: true },
});
