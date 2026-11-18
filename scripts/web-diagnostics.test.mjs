import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../crates/rysmo_app/web_diagnostics.mjs', import.meta.url), 'utf8');

function fixture(supported = true) {
    const elements = [], blobs = [], callbacks = new Map();
    let now = 10, observer;
    const document = {
        body: { append() {} },
        addEventListener() {},
        createElement(tag) {
            const element = { tag, style: {}, dataset: {}, setAttribute() {}, append() {}, remove() {},
                addEventListener(name, callback) { this[name] = callback; }, click() {} };
            elements.push(element);
            return element;
        },
    };
    const window = { rysmoPerformance: { samples: [{ stage: 'test' }], milestones: [], dropped: 2,
        memory: new WebAssembly.Memory({ initial: 1 }) } };
    class Observer {
        static supportedEntryTypes = supported ? ['longtask'] : [];
        constructor(callback) { observer = callback; }
        observe() {}
    }
    vm.runInNewContext(source, { document, window, location: { search: '?diagnostics' }, URLSearchParams,
        performance: { now: () => now }, PerformanceObserver: Observer, WebAssembly, Blob,
        URL: { createObjectURL(blob) { blobs.push(blob); return 'blob:test'; }, revokeObjectURL() {} },
        addEventListener(name, callback) { callbacks.set(name, callback); }, requestAnimationFrame() {}, setTimeout() {},
    });
    return { elements, blobs, window, callbacks, setNow(value) { now = value; },
        entries(entries) { observer({ getEntries: () => entries }); } };
}

test('profile export bounds long tasks and excludes WASM objects from JSON', async () => {
    const f = fixture();
    f.callbacks.get('TrunkApplicationStarted')({ detail: { wasm: { memory: f.window.rysmoPerformance.memory } } });
    f.entries(Array.from({ length: 4100 }, (_, i) => ({ startTime: i + 10, duration: 51 })));
    f.elements.find(e => e.textContent === 'Download profile').click();
    const report = JSON.parse(await f.blobs[0].text());
    assert.equal(report.longTasks.length, 4096);
    assert.equal(report.droppedLongTasks, 4);
    assert.equal(report.wasmBytes, 65536);
    assert.deepEqual(report.samples, [{ stage: 'test' }]);
    assert.equal(report.memory, undefined);
});

test('reset excludes queued earlier observations and unsupported observers remain usable', async () => {
    for (const supported of [true, false]) {
        const f = fixture(supported);
        f.setNow(100);
        f.elements.find(e => e.textContent === 'Reset profile').click();
        if (supported) f.entries([{ startTime: 99, duration: 70 }, { startTime: 101, duration: 60 }]);
        f.elements.find(e => e.textContent === 'Download profile').click();
        const report = JSON.parse(await f.blobs[0].text());
        assert.equal(report.longTasksSupported, supported);
        assert.equal(report.longTasks.length, supported ? 1 : 0);
        assert.equal(report.started, 100);
        assert.deepEqual(report.samples, []);
        assert.equal(report.droppedSamples, 0);
    }
});
