import { renderToStaticMarkup } from 'react-dom/server';
import { ExperimentalSectionTitle } from './shared';

const markup = renderToStaticMarkup(
  ExperimentalSectionTitle({ children: 'Local ASR models', badge: 'Experimental' }),
);

if (!markup.includes('Local ASR models')) {
  throw new Error('experimental section title must render its title');
}
if (!markup.includes('Experimental')) {
  throw new Error('experimental section title must render the shared badge');
}
if ((markup.match(/Experimental/g) ?? []).length !== 1) {
  throw new Error('experimental badge must be rendered exactly once');
}

console.log('experimental section title tests passed');
