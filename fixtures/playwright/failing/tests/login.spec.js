const { test, expect } = require("@playwright/test");

const PAGE = `<!doctype html><title>login</title>
<form id="login"><input id="user"><input id="pass" type="password"><button type="submit">Sign in</button></form>
<p id="status"></p>
<script>
document.getElementById("login").addEventListener("submit", (e) => {
  e.preventDefault();
  const ok = document.getElementById("user").value === "demo" && document.getElementById("pass").value === "secret";
  document.getElementById("status").textContent = ok ? "Welcome, demo" : "Invalid credentials";
});
</script>`;

test("shows the form", async ({ page }) => {
  await page.setContent(PAGE);
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
});

// Seeded bug in the test data: the wrong password is used, so the welcome never appears.
test("signs in with valid credentials", async ({ page }) => {
  await page.setContent(PAGE);
  await page.fill("#user", "demo");
  await page.fill("#pass", "wrong");
  await page.click("button[type=submit]");
  await expect(page.locator("#status")).toHaveText("Welcome, demo", { timeout: 2000 });
});
