import { get, set } from "/__topcoat/storage.js";
import init, {
	createEndpoint,
} from "/__topcoat/topcoat_runtime_iroh_browser.js";
import { forward } from "/__topcoat/transport.js";

let bindings;
let client;

async function connect(address) {
	if (client && client.address !== address) {
		const previous = client;
		client = undefined;
		void previous.ready
			.then(async ({ endpoint, connection }) => {
				connection.close();
				await endpoint.close();
			})
			.catch(() => {});
	}
	if (client) {
		const cached = client;
		const current = await cached.ready;
		if (!current.connection.isClosed()) return current.connection;
		if (client === cached) {
			client = undefined;
			void current.endpoint.close();
		}
		return connect(address);
	}
	bindings ??= init().catch((error) => {
		bindings = undefined;
		throw error;
	});
	const pending = {
		address,
		ready: (async () => {
			await bindings;
			const endpoint = await createEndpoint();
			try {
				return { endpoint, connection: await endpoint.connectHttp(address) };
			} catch (error) {
				await endpoint.close();
				endpoint.free();
				throw error;
			}
		})(),
	};
	client = pending;
	try {
		return (await pending.ready).connection;
	} catch (error) {
		if (client === pending) client = undefined;
		throw error;
	}
}

self.addEventListener("install", (event) =>
	event.waitUntil(self.skipWaiting()),
);
self.addEventListener("activate", (event) =>
	event.waitUntil(self.clients.claim()),
);
self.addEventListener("message", (event) => {
	if (event.data?.type !== "configure" || !event.source || !event.ports[0])
		return;
	if (new URL(event.source.url).origin !== self.location.origin) return;
	const address = event.data.endpoint;
	if (typeof address !== "string" || !address || address.length > 8192) return;
	event.waitUntil(
		(async () => {
			try {
				await connect(address);
				await set("endpoint", address);
				event.ports[0].postMessage({ ok: true });
			} catch (error) {
				event.ports[0].postMessage({ error: String(error) });
			}
		})(),
	);
});

self.addEventListener("fetch", (event) => {
	const url = new URL(event.request.url);
	// Only app resources on this origin belong to the tunnel. The loader,
	// its config and Wasm, and any explicit external URLs use the network.
	if (
		url.origin !== self.location.origin ||
		url.pathname.startsWith("/__topcoat/") ||
		url.pathname === "/topcoat-sw.js"
	)
		return;
	const task = (async () => {
		const address = await get("endpoint");
		if (!address) {
			if (event.request.mode === "navigate") {
				return {
					response: await fetch("/__topcoat/index.html"),
					finished: Promise.resolve(),
				};
			}
			throw new Error("Open the launcher to connect to an app");
		}
		const connection = await connect(address);
		return forward(event.request, connection, {
			get: () => get(`session:${address}`),
			set: (value) => set(`session:${address}`, value),
		});
	})();
	event.respondWith(
		task
			.then(({ response }) => response)
			.catch(
				(error) =>
					new Response(
						`App connection failed: ${String(error)}\nReconnect at /__topcoat/index.html`,
						{
							status: 502,
							headers: {
								"Content-Type": "text/plain; charset=utf-8",
								"Cache-Control": "no-store",
							},
						},
					),
			),
	);
	// Keep the worker alive until a streamed response ends or is cancelled.
	event.waitUntil(task.then(({ finished }) => finished).catch(() => {}));
});
