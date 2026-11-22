local kkc = require("kkc")
local gfx = require("kkc-graphics")
local app = {}
function app.draw()
    gfx.reset()
    gfx.clear()
    gfx.color(0xFFE0A0, 0x000000)
    gfx.text(2, 2, "Hello Kapsule!")
    gfx.text(2, 4, "Lua 5.4 / Rust standalone")
end
function app.keypressed(key)
    if key == "esc" then kkc.quit() end
end
function app.shortcuts()
    return { "F5:Zoom", "Esc:Quit" }
end
return app
