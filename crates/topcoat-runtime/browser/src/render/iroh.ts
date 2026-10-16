import type { OpenSocket, RuntimeSocket } from "./connection";

/** The API exported by the generated Wasm binding module. */
export interface IrohModule {
	default(): Promise<unknown>;
	createEndpoint(): Promise<IrohEndpoint>;
}

export interface IrohEndpoint {
	id(): string;
	connect(address: string, ticket: string): Promise<IrohConnection>;
	close(): Promise<void>;
}

export interface IrohConnection {
	send(message: string): Promise<void>;
	receive(): Promise<string>;
	close(): void;
}

/** Loads the bindings once, then creates a fresh endpoint on each reconnect. */
export function irohTransport(
	moduleUrl: string,
	load: (url: string) => Promise<IrohModule> = (url) =>
		import(/* @vite-ignore */ url),
): OpenSocket {
	let module: Promise<IrohModule> | undefined;
	const bindings = () => {
		module ??= load(moduleUrl)
			.then(async (module) => {
				await module.default();
				return module;
			})
			.catch((error) => {
				module = undefined;
				throw error;
			});
		return module;
	};
	return (url) => new IrohSocket(url, bindings);
}

/** Adapts the asynchronous Wasm stream to the runtime's connection lifecycle. */
class IrohSocket extends EventTarget implements RuntimeSocket {
	readyState = 0;
	private endpoint: IrohEndpoint | undefined;
	private connection: IrohConnection | undefined;
	private readonly abort = new AbortController();
	private sending = Promise.resolve();
	private queuedBytes = 0;
	private readonly encoder = new TextEncoder();

	constructor(url: string, bindings: () => Promise<IrohModule>) {
		super();
		void this.connect(url, bindings).catch((error) => this.fail(error));
	}

	private async connect(url: string, bindings: () => Promise<IrohModule>) {
		const module = await bindings();
		if (this.abort.signal.aborted) return;
		const endpoint = await module.createEndpoint();
		if (this.abort.signal.aborted) {
			await endpoint.close();
			return;
		}
		this.endpoint = endpoint;
		const bootstrapUrl = new URL(url);
		bootstrapUrl.protocol =
			bootstrapUrl.protocol === "wss:" ? "https:" : "http:";
		const response = await fetch(bootstrapUrl, {
			method: "POST",
			credentials: "same-origin",
			redirect: "error",
			cache: "no-store",
			headers: { "Content-Type": "application/json", "X-Topcoat-Iroh": "true" },
			body: JSON.stringify(endpoint.id()),
			signal: this.abort.signal,
		});
		if (!response.ok)
			throw new Error(`Iroh bootstrap failed: ${response.status}`);
		const { address, ticket } = await response.json();
		if (this.abort.signal.aborted) return;
		const connection = await endpoint.connect(JSON.stringify(address), ticket);
		if (this.abort.signal.aborted) {
			connection.close();
			return;
		}
		this.connection = connection;
		this.readyState = 1;
		this.dispatchEvent(new Event("open"));
		while (!this.abort.signal.aborted) {
			const data = await connection.receive();
			if (!this.abort.signal.aborted)
				this.dispatchEvent(new MessageEvent("message", { data }));
		}
	}

	send(message: string): void {
		if (this.readyState !== 1) throw new Error("Iroh connection is closed");
		const bytes = this.encoder.encode(message).byteLength;
		if (
			bytes > 8 * 1024 * 1024 ||
			this.queuedBytes + bytes > 16 * 1024 * 1024
		) {
			this.fail(new Error("Iroh send queue exceeded its limit"));
			return;
		}
		this.queuedBytes += bytes;
		this.sending = this.sending
			.then(async () => {
				if (!this.abort.signal.aborted) await this.connection?.send(message);
			})
			.catch((error) => this.fail(error))
			.finally(() => {
				this.queuedBytes -= bytes;
			});
	}

	close(): void {
		if (this.abort.signal.aborted) return;
		this.abort.abort();
		this.readyState = 3;
		this.connection?.close();
		void this.endpoint?.close().catch(() => {});
		this.dispatchEvent(new Event("close"));
	}

	private fail(error: unknown): void {
		if (this.abort.signal.aborted) return;
		this.dispatchEvent(new CustomEvent("error", { detail: error }));
		this.close();
	}
}
