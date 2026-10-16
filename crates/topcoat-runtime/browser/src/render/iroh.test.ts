// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { type IrohConnection, type IrohEndpoint, irohTransport } from "./iroh";

afterEach(() => vi.unstubAllGlobals());

function fixture() {
	let message: ((value: string) => void) | undefined;
	let disconnected: ((error: Error) => void) | undefined;
	const connection: IrohConnection = {
		send: vi.fn(async () => {}),
		receive: vi.fn(
			() =>
				new Promise<string>((resolve, reject) => {
					message = resolve;
					disconnected = reject;
				}),
		),
		close: vi.fn(() => disconnected?.(new Error("closed"))),
	};
	const endpoint: IrohEndpoint = {
		id: () => "browser-key",
		connect: vi.fn(async () => connection),
		close: vi.fn(async () => {}),
	};
	const init = vi.fn(async () => {});
	const createEndpoint = vi.fn(async () => endpoint);
	const load = vi.fn(async () => ({ default: init, createEndpoint }));
	const fetch = vi.fn(
		async () =>
			new Response(
				JSON.stringify({ address: { id: "server-key" }, ticket: "ticket" }),
			),
	);
	vi.stubGlobal("fetch", fetch);
	const open = irohTransport("/iroh/browser.js", load);
	return {
		connection,
		endpoint,
		load,
		init,
		fetch,
		open,
		receive: (data: string) => message?.(data),
	};
}

it("bootstraps with HTTP credentials and sends the ticket to the advertised peer", async () => {
	const f = fixture();
	const socket = f.open("wss://app.example/room?q=1", "topcoat-runtime");
	const received = vi.fn();
	socket.addEventListener("message", received);
	await vi.waitFor(() => expect(socket.readyState).toBe(1));
	expect(f.fetch).toHaveBeenCalledWith(
		new URL("https://app.example/room?q=1"),
		expect.objectContaining({
			method: "POST",
			credentials: "same-origin",
			redirect: "error",
			headers: { "Content-Type": "application/json", "X-Topcoat-Iroh": "true" },
			body: '"browser-key"',
		}),
	);
	expect(f.endpoint.connect).toHaveBeenCalledWith(
		'{"id":"server-key"}',
		"ticket",
	);
	f.receive('{"run":1,"frame":{"t":"snapshot","html":"hello"}}');
	await vi.waitFor(() => expect(received).toHaveBeenCalledOnce());
	expect((received.mock.calls[0]?.[0] as MessageEvent).data).toContain("hello");
	socket.close();
	expect(f.connection.close).toHaveBeenCalledOnce();
	expect(f.endpoint.close).toHaveBeenCalledOnce();
});

it("serializes writes so stop and replacement runs cannot overtake a pending send", async () => {
	const f = fixture();
	const socket = f.open("ws://app.example/", "topcoat-runtime");
	await vi.waitFor(() => expect(socket.readyState).toBe(1));
	let finish: (() => void) | undefined;
	vi.mocked(f.connection.send).mockImplementationOnce(
		() =>
			new Promise<void>((resolve) => {
				finish = resolve;
			}),
	);
	socket.send("first");
	socket.send("second");
	await vi.waitFor(() => expect(f.connection.send).toHaveBeenCalledTimes(1));
	finish?.();
	await vi.waitFor(() => expect(f.connection.send).toHaveBeenCalledTimes(2));
	expect(vi.mocked(f.connection.send).mock.calls).toEqual([
		["first"],
		["second"],
	]);
	socket.close();
});

it("closes failed bootstraps and reuses the loaded bindings on reconnect", async () => {
	const f = fixture();
	f.fetch.mockResolvedValueOnce(new Response(null, { status: 403 }));
	const first = f.open("ws://app.example/", "topcoat-runtime");
	const errors = vi.fn();
	first.addEventListener("error", errors);
	await vi.waitFor(() => expect(first.readyState).toBe(3));
	expect(errors).toHaveBeenCalledOnce();
	expect(f.endpoint.close).toHaveBeenCalledOnce();
	const next = f.open("ws://app.example/", "topcoat-runtime");
	await vi.waitFor(() => expect(next.readyState).toBe(1));
	expect(f.load).toHaveBeenCalledOnce();
	expect(f.init).toHaveBeenCalledOnce();
	next.close();
});

it("closing during endpoint creation disposes the late endpoint without bootstrapping", async () => {
	const f = fixture();
	let resolve: ((endpoint: IrohEndpoint) => void) | undefined;
	const createEndpoint = vi.fn(
		() =>
			new Promise<IrohEndpoint>((done) => {
				resolve = done;
			}),
	);
	const open = irohTransport("/iroh/browser.js", async () => ({
		default: f.init,
		createEndpoint,
	}));
	const socket = open("ws://app.example/", "topcoat-runtime");
	await vi.waitFor(() => expect(createEndpoint).toHaveBeenCalledOnce());
	socket.close();
	resolve?.(f.endpoint);
	await vi.waitFor(() => expect(f.endpoint.close).toHaveBeenCalledOnce());
	expect(f.fetch).not.toHaveBeenCalled();
	expect(socket.readyState).toBe(3);
});

it("rejects an oversized send instead of buffering it indefinitely", async () => {
	const f = fixture();
	const socket = f.open("ws://app.example/", "topcoat-runtime");
	await vi.waitFor(() => expect(socket.readyState).toBe(1));
	socket.send("x".repeat(8 * 1024 * 1024 + 1));
	expect(socket.readyState).toBe(3);
	expect(f.connection.send).not.toHaveBeenCalled();
});
