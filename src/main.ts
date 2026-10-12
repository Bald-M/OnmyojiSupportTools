import { createApp } from 'vue'

import App from './App.vue'
import { reportFrontendError } from './lib/device'
import './styles.css'

const app = createApp(App)
app.config.errorHandler = () => reportFrontendError('FRONTEND_UNHANDLED')
window.addEventListener('error', () => reportFrontendError('FRONTEND_UNHANDLED'))
window.addEventListener('unhandledrejection', () => reportFrontendError('FRONTEND_REJECTION'))
app.mount('#app')
