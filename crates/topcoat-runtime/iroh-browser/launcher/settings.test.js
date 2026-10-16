import assert from "node:assert/strict";
import test from "node:test";

import { connectionOptions, connectionSettings } from "./settings.js";

const empty = { endpoint: null, ticket: null, secret: null };

test("endpoint and ticket remain separate and are mutually exclusive", () => {
	assert.deepEqual(connectionOptions({ endpoint: " peer " }), {
		endpoint: "peer",
		ticket: "",
		secret: "",
	});
	assert.deepEqual(connectionOptions({ ticket: " ticket " }), {
		endpoint: "",
		ticket: "ticket",
		secret: "",
	});
	assert.throws(() =>
		connectionOptions({ endpoint: "peer", ticket: "ticket" }),
	);
	assert.throws(() => connectionOptions({}));
});

test("a ticket link replaces a configured endpoint without inheriting its identity", () => {
	const saved = { endpoint: "peer", secret: "old identity" };
	assert.deepEqual(
		connectionSettings(
			{ ...empty, ticket: "ticket" },
			{ endpoint: "peer" },
			saved,
		),
		{ endpoint: "", ticket: "ticket", secret: "" },
	);
});

test("an endpoint link replaces a configured ticket", () => {
	assert.deepEqual(
		connectionSettings(
			{ ...empty, endpoint: "peer" },
			{ ticket: "ticket" },
			{},
		),
		{ endpoint: "peer", ticket: "", secret: "" },
	);
});

test("a secret-only link retains the selected destination", () => {
	assert.deepEqual(
		connectionSettings(
			{ ...empty, secret: "identity" },
			{ ticket: "ticket" },
			{},
		),
		{ endpoint: "", ticket: "ticket", secret: "identity" },
	);
});

test("reconnecting retains a saved identity only for the same destination", () => {
	const saved = { endpoint: "peer", ticket: "", secret: "identity" };
	assert.deepEqual(connectionSettings(empty, {}, saved), saved);
	assert.equal(
		connectionSettings(empty, { endpoint: "other" }, saved).secret,
		"",
	);
});
