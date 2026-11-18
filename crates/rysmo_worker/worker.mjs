import init, { import_model, export_skp } from './rysmo_worker.js';

// One worker owns one conversion. Terminating it cancels parsing/encoding too.
self.onmessage = async ({ data }) => {
    try {
        await init();
        if (data.kind === 'import') {
            const document = import_model(data.name, data.bytes, data.scale, data.zUp, data.segments);
            self.postMessage({ document });
        } else if (data.kind === 'export') {
            const output = export_skp(data.document, data.version);
            try {
                const bytes = output.take_bytes();
                const warnings = output.warnings();
                self.postMessage({ bytes, warnings }, [bytes.buffer]);
            } finally {
                output.free();
            }
        } else {
            throw new Error('Unknown conversion job.');
        }
    } catch (error) {
        self.postMessage({ error: String(error) });
    }
};
