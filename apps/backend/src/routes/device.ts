import Router from 'koa-router'
import { screenshot, tap, connect  } from '../controllers/device'
// import { connect } from 'http2'


const router = new Router({ prefix: '/device' })


// GET /device/test
router.get('/test', (ctx) => {
  ctx.body = 'GET /device/test'
})

// 定期侦听接口

// 连接机器接口
router.post('/connect', connect)

router.get('/screenshot', screenshot)

router.post('/tap', tap)

export default router
