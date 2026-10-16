import assert from "node:assert/strict";
import { test } from "node:test";
import { forward } from "./transport.js";

function fixture({ status = 200, headers = [], chunks = [] } = {}) {
	let session = null;
	const state = { uploaded: [], cancelled: 0, freed: 0, ended: false };
	const stream = {
		async write(chunk) {
			state.uploaded.push(...chunk);
		},
		finish() {
			state.ended = true;
		},
		async head() {
			return JSON.stringify({ status, headers });
		},
		async read() {
			return chunks.shift();
		},
		cancel() {
			state.cancelled++;
		},
		free() {
			state.freed++;
		},
	};
	const connection = {
		async open(head) {
			state.head = JSON.parse(head);
			return stream;
		},
	};
	const sessions = {
		async get() {
			return session;
		},
		async set(value) {
			session = value;
		},
	};
	return { state, stream, connection, sessions };
}

test("forwards binary uploads, duplicate headers and streamed response bytes", async () => {
	const f = fixture({
		status: 201,
		headers: [
			["x-many", "one"],
			["x-many", "two"],
		],
		chunks: [new Uint8Array([0, 255]), new Uint8Array([128, 42])],
	});
	const request = new Request("https://app.example/upload?q=1", {
		method: "POST",
		body: new Uint8Array([0, 128, 255]),
		headers: {
			"content-type": "application/octet-stream",
			"accept-encoding": "gzip",
		},
	});
	const { response, finished } = await forward(
		request,
		f.connection,
		f.sessions,
	);
	assert.equal(response.status, 201);
	assert.equal(response.headers.get("x-many"), "one, two");
	assert.deepEqual(
		[...new Uint8Array(await response.arrayBuffer())],
		[0, 255, 128, 42],
	);
	await finished;
	assert.equal(f.state.head.url, request.url);
	assert.deepEqual(f.state.uploaded, [0, 128, 255]);
	assert.ok(!f.state.head.headers.some(([name]) => name === "accept-encoding"));
	assert.equal(f.state.freed, 1);
});

test("forwards payloads when the browser does not expose a request body stream", async () => {
	for (const body of [null, undefined]) {
		for (const payload of ['{"count":1}', new Uint8Array([0, 128, 255])]) {
			const f = fixture();
			const request = new Request("https://app.example/procedure", {
				method: "POST",
				body: payload,
			});
			const expected = new Uint8Array(await request.clone().arrayBuffer());
			Object.defineProperty(request, "body", { value: body });
			const { response, finished } = await forward(
				request,
				f.connection,
				f.sessions,
			);
			await response.arrayBuffer();
			await finished;
			assert.deepEqual(f.state.uploaded, [...expected]);
			assert.equal(f.state.ended, true);
			assert.equal(f.state.freed, 1);
		}
	}
});

test("persists session changes before redirects and sends credentials on navigation", async () => {
	const f = fixture({
		status: 303,
		headers: [
			["location", "/account"],
			["x-topcoat-session", JSON.stringify({ token: "stale", maxAge: 60 })],
			["x-topcoat-session", JSON.stringify({ token: "secret", maxAge: 60 })],
		],
	});
	const { response, finished } = await forward(
		new Request("https://app.example/login", { method: "POST" }),
		f.connection,
		f.sessions,
	);
	assert.equal(response.status, 303);
	assert.equal(response.headers.get("location"), "/account");
	assert.equal(response.headers.get("x-topcoat-session"), null);
	await response.arrayBuffer();
	await finished;
	f.stream.head = async () => JSON.stringify({ status: 200, headers: [] });
	const next = await forward(
		new Request("https://app.example/account"),
		f.connection,
		f.sessions,
	);
	assert.ok(
		f.state.head.headers.some(
			([name, value]) => name === "authorization" && value === "Bearer secret",
		),
	);
	await next.response.arrayBuffer();
	await next.finished;
});

test("omitted or expired credentials are not forwarded", async () => {
	for (const [credentials, expires] of [
		["omit", Date.now() + 60_000],
		["same-origin", Date.now() - 1],
	]) {
		const f = fixture({ status: 204 });
		await f.sessions.set({ token: "secret", expires });
		const result = await forward(
			new Request("https://app.example/", { credentials }),
			f.connection,
			f.sessions,
		);
		assert.equal(result.response.body, null);
		await result.finished;
		assert.ok(!f.state.head.headers.some(([name]) => name === "authorization"));
	}
});

test("response cancellation stops the iroh stream", async () => {
	const f = fixture({ chunks: [new Uint8Array([1])] });
	const { response, finished } = await forward(
		new Request("https://app.example/"),
		f.connection,
		f.sessions,
	);
	await response.body.cancel();
	await finished;
	assert.equal(f.state.cancelled, 1);
	assert.equal(f.state.freed, 1);
});

test("abort interrupts pending headers and frees only after the read settles", async () => {
	const f = fixture();
	let rejectHead;
	f.stream.head = () =>
		new Promise((_, reject) => {
			rejectHead = reject;
		});
	f.stream.cancel = () => {
		f.state.cancelled++;
		rejectHead?.(new Error("cancelled"));
	};
	const abort = new AbortController();
	const pending = forward(
		new Request("https://app.example/", { signal: abort.signal }),
		f.connection,
		f.sessions,
	);
	await new Promise((resolve) => setImmediate(resolve));
	abort.abort();
	await assert.rejects(pending);
	await new Promise((resolve) => setImmediate(resolve));
	assert.equal(f.state.freed, 1);
});

test("cookie responses fail explicitly instead of silently losing login state", async () => {
	const f = fixture({ headers: [["set-cookie", "session=secret; HttpOnly"]] });
	await assert.rejects(
		forward(new Request("https://app.example/"), f.connection, f.sessions),
	);
	await new Promise((resolve) => setImmediate(resolve));
	assert.equal(f.state.cancelled, 1);
	assert.equal(f.state.freed, 1);
});
