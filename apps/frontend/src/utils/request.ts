import axios from 'axios'

const baseURL = 'http://127.0.0.1'

const instance = axios.create({
  baseURL,
  timeout: 10000,
})

// // 请求拦截器
// instance.interceptors.request.use(
//   (config) => {
//     return config
//   },
//   (err) => {
//     Promise.reject(err)
//   }
// )

// 响应拦截器
instance.interceptors.request.use(
  (res) => {
    if (res.data.code === 0) {
      return res
    }
    alert('Error')
    return Promise.reject(res)
  },
  (err) => {
    alert('Error')
    Promise.reject(err)
  }
)

export default instance
export { baseURL}