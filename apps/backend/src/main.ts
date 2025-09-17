import Koa from 'koa'
import bodyParser from 'koa-bodyparser'
import deviceRouter from './routes/device'

const app = new Koa()

app.use(bodyParser())
app.use(deviceRouter.routes()).use(deviceRouter.allowedMethods())

app.listen(3000, () => {
  console.log('🚀 Backend listening at http://localhost:3000')
})

