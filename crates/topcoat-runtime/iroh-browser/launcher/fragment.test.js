import assert from "node:assert/strict";
import test from "node:test";

import { connectionFragment } from "./fragment.js";

test("connection fields are decoded and removed without discarding app fields", () => {
	const result = connectionFragment(
		"https://app.example/notes?sort=asc#ticket=a%2Bb&secret=key&tab=files",
	);
	assert.equal(result.endpoint, null);
	assert.equal(result.ticket, "a+b");
	assert.equal(result.secret, "key");
	assert.equal(result.href, "https://app.example/notes?sort=asc#tab=files");
});

test("endpoint and ticket remain separate for connection validation", () => {
	const result = connectionFragment(
		"https://app.example/#iroh=old&endpoint=id&ticket=ticket",
	);
	assert.equal(result.endpoint, "id");
	assert.equal(result.ticket, "ticket");
	assert.equal(result.secret, null);
	assert.equal(result.href, "https://app.example/");
});

test("secret can select an identity without overriding the configured endpoint", () => {
	const result = connectionFragment("https://app.example/#secret=key");
	assert.equal(result.present, true);
	assert.equal(result.endpoint, null);
	assert.equal(result.secret, "key");
});

test("ordinary app anchors are preserved exactly", () => {
	const href = "https://app.example/notes#chapter%202";
	const result = connectionFragment(href);
	assert.equal(result.present, false);
	assert.equal(result.href, href);
});
