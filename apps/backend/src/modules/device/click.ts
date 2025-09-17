import { adbShell } from '../../utils/adb';

export async function tap(x: number, y: number): Promise<void> {
  await adbShell(['input', 'tap', String(x), String(y)]);
}
