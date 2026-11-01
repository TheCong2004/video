with open(r"d:\capcutpolot\postiz-app\apps\frontend\.next\server\app\launches.html", "r", encoding="utf-8") as f:
    orig = f.read()

# Xem doan sau pt-[100px]
idx = orig.find('pt-[100px]')
print(orig[idx-200:idx+400])

