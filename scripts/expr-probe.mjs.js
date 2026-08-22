return { title: document.title, hasCard: !!document.querySelector("[class*=rounded-xl]"), body: document.body.innerText.slice(0, 200) };
