import Koa from 'koa'
import Router from 'koa-router'
import bodyParser from 'koa-bodyparser'
import deviceRouter from './routes/device'

const app = new Koa()
const router = new Router()

router.get('/', (ctx) => {
  ctx.body = 'Hello from Koa backend!'
})

app.use(bodyParser())
app.use(router.routes())
app.use(router.allowedMethods())

// ✅ 将 deviceRouter 挂载到 /device 前缀下
router.use('/device', deviceRouter.routes(), deviceRouter.allowedMethods())

app.listen(3000, () => {
  console.log('🚀 Backend listening at http://localhost:3000')
})
