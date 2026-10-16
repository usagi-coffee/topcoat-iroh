import { connectionOptions } from "/__topcoat/settings.js";
import { get, set } from "/__topcoat/storage.js";
import init, {
	createEndpoint,
} from "/__topcoat/topcoat_runtime_iroh_browser.js";
import { forward } from "/__topcoat/transport.js";

let bindings;
let client;

async function connect({ endpoint, ticket, secret }) {
	if (
		client &&
		(client.endpoint !== endpoint ||
			client.ticket !== ticket ||
			client.secret !== secret)
	) {
		const previous = client;
		client = undefined;
		void previous.ready
			.then(async ({ clientEndpoint, connection }) => {
				connection.close();
				await clientEndpoint.close();
			})
			.catch(() => {});
	}
	if (client) {
		const cached = client;
		const current = await cached.ready;
		if (!current.connection.isClosed()) return current;
		if (client === cached) {
			client = undefined;
			void current.clientEndpoint.close();
		}
		return connect({ endpoint, ticket, secret });
	}
	bindings ??= init().catch((error) => {
		bindings = undefined;
		throw error;
	});
	const pending = {
		endpoint,
		ticket,
		secret,
		ready: (async () => {
			await bindings;
			const clientEndpoint = await createEndpoint(secret || undefined);
			try {
				return {
					clientEndpoint,
					connection: await clientEndpoint.connectHttp(
						endpoint || undefined,
						ticket || undefined,
					),
				};
			} catch (error) {
				await clientEndpoint.close();
				clientEndpoint.free();
				throw error;
			}
		})(),
	};
	client = pending;
	try {
		return await pending.ready;
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
let configuring = Promise.resolve();
self.addEventListener("message", (event) => {
	if (event.data?.type !== "configure" || !event.source || !event.ports[0])
		return;
	if (new URL(event.source.url).origin !== self.location.origin) return;
	configuring = configuring.then(async () => {
		try {
			const settings = connectionOptions(event.data);
			const { clientEndpoint } = await connect(settings);
			await set("connection", settings);
			event.ports[0].postMessage({ ok: true, endpointId: clientEndpoint.id() });
		} catch (error) {
			event.ports[0].postMessage({ error: String(error) });
		}
	});
	event.waitUntil(configuring);
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
		const settings = (await get("connection")) ?? {
			endpoint: (await get("endpoint")) ?? "",
			ticket: "",
			secret: "",
		};
		const { endpoint, ticket, secret } = settings;
		if (!endpoint && !ticket) {
			if (event.request.mode === "navigate") {
				return {
					response: await fetch("/__topcoat/index.html"),
					finished: Promise.resolve(),
				};
			}
			throw new Error("Open the launcher to connect to an app");
		}
		const { connection, clientEndpoint } = await connect(
			connectionOptions(settings),
		);
		let sessionKey = ticket
			? `session:ticket:${ticket}`
			: `session:${endpoint}`;
		if (secret) sessionKey += `:${clientEndpoint.id()}`;
		return forward(event.request, connection, {
			get: () => get(sessionKey),
			set: (value) => set(sessionKey, value),
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
