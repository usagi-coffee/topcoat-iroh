let database;

async function open() {
	database ??= new Promise((resolve, reject) => {
		const request = indexedDB.open("topcoat-iroh", 1);
		request.onupgradeneeded = () =>
			request.result.createObjectStore("settings");
		request.onsuccess = () => resolve(request.result);
		request.onerror = () => reject(request.error);
	}).catch((error) => {
		database = undefined;
		throw error;
	});
	return database;
}

export async function get(key) {
	const db = await open();
	return new Promise((resolve, reject) => {
		const request = db.transaction("settings").objectStore("settings").get(key);
		request.onsuccess = () => resolve(request.result);
		request.onerror = () => reject(request.error);
	});
}

export async function set(key, value) {
	const db = await open();
	return new Promise((resolve, reject) => {
		const transaction = db.transaction("settings", "readwrite");
		transaction.objectStore("settings").put(value, key);
		transaction.oncomplete = () => resolve();
		transaction.onabort = () => reject(transaction.error);
		transaction.onerror = () => reject(transaction.error);
	});
}
