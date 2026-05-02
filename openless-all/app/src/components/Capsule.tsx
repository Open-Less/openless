import { detectOS } from './WindowChrome';
import { SharedCapsule } from './SharedCapsule';
import { WindowsCapsule } from './WindowsCapsule';

export function Capsule() {
  const os = detectOS();
  return os === 'win' ? <WindowsCapsule /> : <SharedCapsule />;
}
