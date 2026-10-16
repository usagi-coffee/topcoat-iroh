import { connect, openApp } from "/__topcoat/connect.js";
import { connectionFragment } from "/__topcoat/fragment.js";
import { connectionSettings } from "/__topcoat/settings.js";
import { get } from "/__topcoat/storage.js";

const status = document.querySelector("#status");
const endpointInput = document.querySelector("#endpoint");
const ticketInput = document.querySelector("#ticket");
const button = document.querySelector("button");
const fragment = connectionFragment(location.href);
if (fragment.present) history.replaceState(history.state, "", fragment.href);
let secret = fragment.secret ?? "";

async function start() {
	button.disabled = true;
	status.textContent = "Connecting...";
	try {
		await connect({
			endpoint: endpointInput.value,
			ticket: ticketInput.value,
			secret,
		});
		openApp();
	} catch (error) {
		status.textContent = String(error);
		button.disabled = false;
	}
}

document.querySelector("form").addEventListener("submit", (event) => {
	event.preventDefault();
	void start();
});

try {
	const config = await fetch("/__topcoat/config.json", {
		cache: "no-store",
	}).then((response) => {
		if (!response.ok) throw new Error("Cannot load launcher config");
		return response.json();
	});
	const saved = (await get("connection")) ?? {
		endpoint: (await get("endpoint")) ?? "",
	};
	const settings = connectionSettings(fragment, config, saved);
	endpointInput.value = settings.endpoint;
	ticketInput.value = settings.ticket;
	secret = settings.secret;
	status.textContent = "Enter an endpoint or a ticket for the running app.";
	if (
		(settings.endpoint || settings.ticket) &&
		(fragment.present || !location.pathname.startsWith("/__topcoat/"))
	)
		await start();
} catch (error) {
	status.textContent = String(error);
}
