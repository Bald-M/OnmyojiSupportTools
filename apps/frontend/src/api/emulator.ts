import request from '@/utils/request'

export const emulatorConnectService = (ip: string, port: string) => {
  return request.post('/device/connect', {ip, port})
}

export const emulatorScreenshotService = () => {
  return request.get('/device/screenshot')
}