import { spawn, execSync } from 'child_process';

export function adbShell(args: string[] = ['screencap', '-p']): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const adbPath = 'D:/Program Files (x86)/MuMu Player 12/shell/adb.exe';
    const deviceAddr = '127.0.0.1:16384';

    try {
      // 确保已连接设备
      const devicesOutput = execSync(`"${adbPath}" devices`).toString();
      if (!devicesOutput.includes(deviceAddr)) {
        console.log(`🔌 Connecting to ${deviceAddr}...`);
        execSync(`"${adbPath}" connect ${deviceAddr}`);
      }
    } catch (e) {
      return reject(`ADB connect failed: ${e}`);
    }

    // 执行 ADB 命令
    const adb = spawn(adbPath, ['-s', deviceAddr, 'shell', ...args]);
    const chunks: Buffer[] = [];
    const errors: Buffer[] = [];

    adb.stdout.on('data', (data) => chunks.push(data));
    adb.stderr.on('data', (err) => errors.push(err));

    adb.on('close', (code) => {
      if (code === 0) resolve(Buffer.concat(chunks));
      else reject(Buffer.concat(errors).toString());
    });
  });
}
