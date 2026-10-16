// Shared by connection screens rendered with Topcoat and custom browser code.
import { connectionFragment } from "/__topcoat/fragment.js";
import { connectionOptions } from "/__topcoat/settings.js";

let pending;

export function connect(options) {
	if (pending)
		return Promise.reject(new Error("A connection is already pending"));
	pending = configureOptions(options).finally(() => {
		pending = undefined;
	});
	return pending;
}

async function configureOptions(options) {
	const endpoint = options.endpoint ?? "";
	const ticket = options.ticket ?? "";
	if (typeof endpoint !== "string" || typeof ticket !== "string")
		throw new Error("Connection fields must be strings");
	if (!endpoint.trim() && !ticket.trim()) {
		const response = await fetch("/__topcoat/config.json", {
			cache: "no-store",
		});
		if (!response.ok) throw new Error("Cannot load connection config");
		const config = await response.json();
		options = {
			endpoint: config.endpoint ?? "",
			ticket: config.ticket ?? "",
			secret: options.secret ?? "",
		};
	}
	return configure(connectionOptions(options));
}

async function configure({ endpoint, ticket, secret }) {
	if (!isSecureContext || !navigator.serviceWorker)
		throw new Error("Open this launcher over HTTPS (or localhost).");
	const registered = await navigator.serviceWorker.register("/topcoat-sw.js", {
		type: "module",
		scope: "/",
		updateViaCache: "none",
	});
	const installing = registered.installing ?? registered.waiting;
	if (installing && installing.state !== "activated") {
		await new Promise((resolve, reject) => {
			const changed = () => {
				if (
					installing.state !== "activated" &&
					installing.state !== "redundant"
				)
					return;
				installing.removeEventListener("statechange", changed);
				if (installing.state === "activated") resolve();
				else reject(new Error("The launcher service worker could not start"));
			};
			installing.addEventListener("statechange", changed);
			changed();
		});
	}
	const registration = await navigator.serviceWorker.ready;
	if (!navigator.serviceWorker.controller) {
		await new Promise((resolve) =>
			navigator.serviceWorker.addEventListener("controllerchange", resolve, {
				once: true,
			}),
		);
	}
	return new Promise((resolve, reject) => {
		const channel = new MessageChannel();
		const timeout = setTimeout(() => {
			channel.port1.close();
			reject(new Error("Connection timed out. Check that the app is running."));
		}, 45_000);
		channel.port1.onmessage = ({ data }) => {
			clearTimeout(timeout);
			channel.port1.close();
			if (data.error) reject(new Error(data.error));
			else resolve({ endpointId: data.endpointId });
		};
		registration.active.postMessage(
			{ type: "configure", endpoint, ticket, secret },
			[channel.port2],
		);
	});
}

export function openApp() {
	const path = location.pathname.startsWith("/__topcoat/")
		? "/"
		: location.pathname + location.search;
	const fragment = new URL(connectionFragment(location.href).href).hash;
	const target = new URL(path + fragment, location.href);
	if (
		target.pathname === location.pathname &&
		target.search === location.search
	) {
		// Replacing only a fragment does not fetch the app through the new worker.
		history.replaceState(history.state, "", target.href);
		location.reload();
	} else {
		location.replace(target.href);
	}
}
