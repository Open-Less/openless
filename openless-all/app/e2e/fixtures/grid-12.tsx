import '../../src/styles/tokens.css';
import '../../src/styles/grid.css';

const cells = (count: number, className = '') =>
  Array.from({ length: count }, (_, index) => `<div class="${className}">${index + 1}</div>`).join(
    '',
  );

document.querySelector('#root')!.innerHTML = `
  <main style="width:960px;padding:24px;display:grid;gap:16px">
    <section id="reference" class="ol-grid-12">${cells(12, 'ol-col-1')}</section>
    <section id="cards" class="ol-grid-12 ol-grid-cards">${cells(3)}</section>
    <section id="stats" class="ol-grid-12 ol-grid-stats">${cells(4)}</section>
    <section id="quarters" class="ol-grid-12 ol-grid-quarters">${cells(4)}</section>
    <section id="meta" class="ol-grid-12 ol-grid-meta">${cells(3)}</section>
    <section id="fields" class="ol-grid-12 ol-grid-fields">${cells(2)}</section>
    <section id="form" class="ol-form-12"><div>Label</div><div>Control</div></section>
    <section id="pipeline" class="ol-grid-12 ol-pipeline-grid">${cells(6)}</section>
    <section id="bands" class="ol-grid-12 ol-grid-bands">${cells(10)}</section>
    <section id="composer" class="ol-grid-12">
      <div class="ol-col-5">Pattern</div>
      <div class="ol-col-1">→</div>
      <div class="ol-col-4">Replacement</div>
      <div class="ol-col-2">Add</div>
    </section>
    <section id="split" class="ol-grid-12">
      <div class="ol-col-5">Left</div>
      <div class="ol-col-7">Right</div>
    </section>
    <section id="forced-one" class="ol-grid-12" data-columns="1">
      <div class="ol-col-4">Full</div>
    </section>
    <section id="stacked-cards" class="ol-grid-12 ol-grid-cards">${cells(3)}</section>
  </main>
`;
