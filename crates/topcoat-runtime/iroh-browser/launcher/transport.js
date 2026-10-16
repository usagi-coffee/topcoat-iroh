/** Adapt one Wasm request stream to the browser's streaming Response API. */
export async function forward(request, connection, sessions) {
	const headers = new Headers(request.headers);
	for (const name of [
		"cookie",
		"host",
		"accept-encoding",
		"content-length",
		"connection",
		"upgrade",
		"transfer-encoding",
	]) {
		headers.delete(name);
	}
	const credentials = request.credentials !== "omit";
	if (credentials && !headers.has("authorization")) {
		const session = await sessions.get();
		if (session && session.expires > Date.now()) {
			headers.set("authorization", `Bearer ${session.token}`);
		}
	}
	const stream = await connection.open(
		JSON.stringify({
			method: request.method,
			url: request.url,
			headers: [...headers],
		}),
	);
	let finish;
	const finished = new Promise((resolve) => {
		finish = resolve;
	});
	let disposed = false;
	let reading = Promise.resolve();
	let writing = Promise.resolve();
	const upload = request.body?.getReader();
	const dispose = () => {
		if (disposed) return;
		disposed = true;
		request.signal.removeEventListener("abort", dispose);
		stream.cancel();
		// Wasm exports cannot be freed while an async method still borrows them.
		void upload?.cancel().catch(() => {});
		void Promise.allSettled([reading, writing]).then(() => {
			stream.free();
			finish();
		});
	};
	request.signal.addEventListener("abort", dispose, { once: true });
	if (request.signal.aborted) {
		dispose();
		throw request.signal.reason;
	}
	writing = (async () => {
		if (upload) {
			while (true) {
				const { done, value } = await upload.read();
				if (done) break;
				await stream.write(value);
			}
		}
		stream.finish();
	})();
	// A handler can return early without reading the whole upload. Its response
	// is still valid when the peer stops accepting the request body.
	void writing.catch(() => {});
	try {
		reading = stream.head();
		const head = JSON.parse(await reading);
		const responseHeaders = new Headers(head.headers);
		if (responseHeaders.has("set-cookie")) {
			throw new Error(
				"The static launcher cannot set HTTP cookies. Use HeaderTokenStore for sessions.",
			);
		}
		const session = head.headers
			.filter(([name]) => name.toLowerCase() === "x-topcoat-session")
			.at(-1)?.[1];
		if (session !== undefined && credentials) {
			const value = JSON.parse(session);
			if (
				typeof value.token !== "string" ||
				!Number.isSafeInteger(value.maxAge) ||
				value.maxAge < 0
			) {
				throw new Error("Invalid session response");
			}
			await sessions.set(
				value.maxAge === 0
					? null
					: {
							token: value.token,
							expires: Date.now() + value.maxAge * 1000,
						},
			);
		}
		responseHeaders.delete("x-topcoat-session");
		// These describe HTTP framing, which the iroh protocol does not use.
		responseHeaders.delete("transfer-encoding");
		responseHeaders.delete("connection");
		if (responseHeaders.has("content-encoding")) {
			throw new Error(
				"The static launcher requires uncompressed response bodies",
			);
		}
		const noBody =
			request.method === "HEAD" || [204, 205, 304].includes(head.status);
		let body = null;
		if (noBody) {
			dispose();
		} else {
			body = new ReadableStream({
				async pull(controller) {
					try {
						reading = stream.read();
						const chunk = await reading;
						if (chunk === undefined) {
							controller.close();
							dispose();
						} else {
							controller.enqueue(chunk);
						}
					} catch (error) {
						controller.error(error);
						dispose();
					}
				},
				cancel: dispose,
			});
		}
		return {
			response: new Response(body, {
				status: head.status,
				headers: responseHeaders,
			}),
			finished,
		};
	} catch (error) {
		dispose();
		throw error;
	}
}
