import assert from "node:assert/strict";

// Run against the iroh-app example and a fresh Chromium profile with remote debugging.
const debuggerUrl = process.env.IROH_TEST_DEBUGGER || "http://127.0.0.1:9398";
const appUrl = process.env.IROH_TEST_URL || "http://127.0.0.1:3198";
assert.ok(
	process.env.IROH_TEST_PEER,
	"Set IROH_TEST_PEER to the example endpoint ID",
);
const targets = await (await fetch(`${debuggerUrl}/json/list`)).json();
const page = targets.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve) =>
	ws.addEventListener("open", resolve, { once: true }),
);
let id = 0;
let loads = 0;
const pending = new Map();
const errors = [];
const responses = [];
ws.addEventListener("message", (event) => {
	const message = JSON.parse(event.data);
	if (message.method === "Page.loadEventFired") loads++;
	if (message.id) {
		pending.get(message.id)?.(message);
		pending.delete(message.id);
	}
	if (message.method === "Runtime.exceptionThrown")
		errors.push(message.params.exceptionDetails);
	if (message.method === "ServiceWorker.workerErrorReported")
		errors.push(message.params);
	if (message.method === "Network.responseReceived")
		responses.push({
			url: message.params.response.url,
			status: message.params.response.status,
			worker: message.params.response.fromServiceWorker,
		});
});
const send = (method, params = {}) =>
	new Promise((resolve) => {
		const key = ++id;
		pending.set(key, resolve);
		ws.send(JSON.stringify({ id: key, method, params }));
	});
const evaluate = async (expression) => {
	const r = await send("Runtime.evaluate", {
		expression,
		returnByValue: true,
		awaitPromise: true,
	});
	if (r.result?.exceptionDetails)
		throw new Error(JSON.stringify(r.result.exceptionDetails));
	return r.result?.result?.value;
};
const waitFor = async (expression) => {
	for (let i = 0; i < 60; i++) {
		if (await evaluate(expression)) return;
		await new Promise((resolve) => setTimeout(resolve, 1000));
	}
	throw new Error(
		"Timeout: " +
			expression +
			"; body=" +
			(await evaluate("document.body?.innerText")) +
			"; errors=" +
			JSON.stringify(errors),
	);
};
await send("Page.enable");
await send("Runtime.enable");
await send("Network.enable");
await send("ServiceWorker.enable");
const reload = async () => {
	const previous = loads;
	await send("Page.reload");
	for (let attempt = 0; attempt < 60; attempt++) {
		if (loads > previous) return;
		await new Promise((resolve) => setTimeout(resolve, 1000));
	}
	throw new Error("Reload did not complete");
};
try {
	const mode = process.env.IROH_TEST_MODE || "legacy";
	const ticket = process.env.IROH_TEST_TICKET;
	if (mode === "custom" || mode === "identity")
		assert.ok(ticket, "Set IROH_TEST_TICKET to the example endpoint ticket");
	const secret =
		"9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
	const fragment =
		mode === "custom"
			? ""
			: mode === "identity"
				? `#ticket=${encodeURIComponent(ticket)}&secret=${secret}&tab=files`
				: mode === "configured"
					? ""
					: `#endpoint=${process.env.IROH_TEST_PEER}`;
	await send("Page.navigate", { url: `${appUrl}/${fragment}` });
	if (mode === "denied") {
		await waitFor(
			'document.body?.textContent.includes("App connection failed") || (document.querySelector("[role=status]")?.textContent.includes("Error"))',
		);
		assert.equal(await evaluate('!!document.querySelector("#tick")'), false);
		console.log("Unlisted client was denied before the app loaded");
	} else {
		await waitFor(
			'document.readyState === "complete" && (document.querySelector("input#ticket") || document.querySelector("#tick"))',
		);
		const customLauncher = mode === "custom";
		if (customLauncher) {
			assert.equal(
				await evaluate('document.querySelector("input#endpoint").value'),
				process.env.IROH_TEST_PEER,
			);
			assert.equal(
				await evaluate('document.querySelector("input#ticket").value'),
				"",
			);
			const fill = async (name, value) =>
				evaluate(
					`{ const input = document.querySelector('input#${name}'); input.value = ${JSON.stringify(value)}; input.dispatchEvent(new Event('input', { bubbles: true })); }`,
				);
			await fill("ticket", ticket);
			await fill("endpoint", process.env.IROH_TEST_PEER);
			await evaluate('document.querySelector("form").requestSubmit()');
			await waitFor(
				'document.querySelector("[role=status]")?.textContent.includes("either an endpoint or a ticket")',
			);
			await fill("endpoint", "");
			await fill("secret", "invalid-secret");
			await evaluate('document.querySelector("form").requestSubmit()');
			await waitFor(
				'document.querySelector("[role=status]")?.textContent && document.querySelector("[role=status]").textContent !== "Connecting..." && !document.querySelector("[role=status]").textContent.includes("either an endpoint or a ticket")',
			);
			await fill("secret", secret);
			await evaluate('document.querySelector("form").requestSubmit()');
		}
		await waitFor(
			'document.querySelector("#tick")?.textContent.match(/Tick: [1-9]/)',
		);
		const peer = await evaluate('document.querySelector("#peer")?.textContent');
		if (customLauncher || mode === "identity")
			assert.equal(
				peer,
				"d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
			);
		if (mode === "identity")
			assert.equal(await evaluate("location.hash"), "#tab=files");
		if (mode === "automatic" || mode === "configured")
			assert.match(peer, /^[a-f0-9]{64}$/);
		console.log("Initial page and live updates arrived");
		assert.notEqual(
			await evaluate("getComputedStyle(document.body).maxWidth"),
			"none",
		);
		await evaluate('document.querySelector("#double").click()');
		await waitFor('document.querySelector("#value")?.textContent === "4"');
		await evaluate(
			'document.querySelector("#name").value="across networks"; document.querySelector("#name").dispatchEvent(new Event("input",{bubbles:true}))',
		);
		await waitFor(
			'document.querySelector("#greeting")?.textContent.includes("across networks")',
		);
		assert.deepEqual(
			await evaluate(
				'(async()=> { const r=await fetch("/echo", {method:"POST",body:new Uint8Array([0,128,255])}); return [...new Uint8Array(await r.arrayBuffer())]; })()',
			),
			[0, 128, 255],
		);
		console.log("Stylesheet, procedure, shard, binary upload arrived");
		await evaluate(
			'document.querySelector("form[action=\\"/login\\"]").requestSubmit()',
		);
		await waitFor(
			'document.querySelector("#session")?.textContent === "Session: active"',
		);
		await send("ServiceWorker.stopAllWorkers");
		await reload();
		await waitFor(
			'document.querySelector("#session")?.textContent === "Session: active"',
		);
		if (customLauncher || mode === "identity")
			assert.equal(
				await evaluate('document.querySelector("#peer")?.textContent'),
				peer,
			);
		await send("Page.navigate", { url: `${appUrl}/next` });
		await waitFor(
			'document.querySelector("h1")?.textContent === "Another page over iroh"',
		);
		await send("Page.navigate", { url: `${appUrl}/` });
		await waitFor(
			'document.querySelector("#session")?.textContent === "Session: active"',
		);
		await evaluate(
			'document.querySelector("form[action=\\"/logout\\"]").requestSubmit()',
		);
		await waitFor(
			'document.querySelector("#session")?.textContent === "Session: none"',
		);
		console.log(
			"Forms, redirects, sessions after worker restart, navigation, and logout worked",
		);
		assert.equal(errors.length, 0, JSON.stringify(errors));
		assert.ok(
			responses.some(
				(response) => response.worker && response.url.endsWith("/next"),
			),
		);
		console.log(
			"App routes and assets were served by the worker; no browser exceptions.",
		);
	}
} finally {
	ws.close();
}
