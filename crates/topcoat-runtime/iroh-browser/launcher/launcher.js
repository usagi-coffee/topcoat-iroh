import { get } from "/__topcoat/storage.js";

const status = document.querySelector("#status");
const input = document.querySelector("#endpoint");
const button = document.querySelector("button");
let worker;

async function start(address) {
	button.disabled = true;
	status.textContent = "Connecting...";
	try {
		await new Promise((resolve, reject) => {
			const channel = new MessageChannel();
			const timeout = setTimeout(() => {
				channel.port1.close();
				reject(
					new Error("Connection timed out. Check that the app is running."),
				);
			}, 45_000);
			channel.port1.onmessage = ({ data }) => {
				clearTimeout(timeout);
				channel.port1.close();
				if (data.error) reject(new Error(data.error));
				else resolve();
			};
			worker.postMessage({ type: "configure", endpoint: address }, [
				channel.port2,
			]);
		});
		// Preserve deep links when a static host served its 404 launcher.
		const target = location.pathname.startsWith("/__topcoat/")
			? "/"
			: location.pathname + location.search;
		const fragment = location.hash.startsWith("#iroh=") ? "" : location.hash;
		location.replace(target + fragment);
	} catch (error) {
		status.textContent = String(error);
		button.disabled = false;
	}
}

document.querySelector("form").addEventListener("submit", (event) => {
	event.preventDefault();
	if (worker) void start(input.value.trim());
});

try {
	if (!isSecureContext || !navigator.serviceWorker)
		throw new Error("Open this launcher over HTTPS (or localhost).");
	const registration = await navigator.serviceWorker.register(
		"/topcoat-sw.js",
		{ type: "module", scope: "/" },
	);
	await navigator.serviceWorker.ready;
	if (!navigator.serviceWorker.controller) {
		await new Promise((resolve) =>
			navigator.serviceWorker.addEventListener("controllerchange", resolve, {
				once: true,
			}),
		);
	}
	worker = registration.active;
	const config = await fetch("/__topcoat/config.json", {
		cache: "no-store",
	}).then((response) => {
		if (!response.ok) throw new Error("Cannot load launcher config");
		return response.json();
	});
	const fragment = new URLSearchParams(location.hash.slice(1));
	input.value =
		fragment.get("iroh") || config.endpoint || (await get("endpoint")) || "";
	status.textContent = "Enter the address of the running app.";
	if (input.value && !location.pathname.startsWith("/__topcoat/"))
		await start(input.value);
} catch (error) {
	status.textContent = String(error);
}
