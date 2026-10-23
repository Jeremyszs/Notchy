import { test } from "node:test";
import assert from "node:assert/strict";
import { accessCopy, initialOf, isAllowed, setAllowed } from "./notifications.ts";

test("the badge is the app's first letter, upper-cased, and a dot when there is no name", () => {
  assert.equal(initialOf("mail"), "M");
  assert.equal(initialOf("  Teams  "), "T");
  assert.equal(initialOf("édition"), "É");
  assert.equal(initialOf(""), "•");
  assert.equal(initialOf("   "), "•");
});

test("each access state says what to do, and only denied and unspecified point at Windows", () => {
  assert.match(accessCopy("allowed"), /lets Notchy read/);
  assert.match(accessCopy("denied"), /Privacy & security > Notifications/);
  assert.match(accessCopy("unspecified"), /has not decided yet/);
  assert.match(accessCopy("unknown"), /Checking/);
});

test("an app is shown unless its id is in the denied list, whatever the case", () => {
  assert.equal(isAllowed("Contoso.Mail_1!App", []), true);
  assert.equal(isAllowed("Contoso.Mail_1!App", ["Contoso.Mail_1!App"]), false);
  assert.equal(isAllowed("CHROME", ["chrome"]), false);
  assert.equal(isAllowed("Teams", ["Chrome"]), true);
});

test("allowing an app removes it from the denied list and denying adds it once", () => {
  const denied = ["Chat"];
  assert.deepEqual(setAllowed(denied, "Chat", true), []);
  assert.deepEqual(setAllowed(denied, "Mail", false), ["Chat", "Mail"]);
  assert.deepEqual(setAllowed(["Mail"], "mail", false), ["mail"]);
  assert.deepEqual(setAllowed(["Chat"], "Chat", false), ["Chat"]);
});

test("the denied list is not changed in place", () => {
  const denied = ["Chat"];
  setAllowed(denied, "Mail", false);
  assert.deepEqual(denied, ["Chat"]);
});
