const DATA_COUNT = 7*24;
const last_generated = /*BEGIN-LAST-GENERATED*/new Date().getTime()/*END-LAST-GENERATED*/;
const labels = [];
for (let d = DATA_COUNT; d >= 0; d--) {
    let date2 = new Date(last_generated - d * 3600 * 1000);
    let date_fmt_french = date2.toLocaleDateString('fr-FR', {weekday: 'short', hour: '2-digit'});
    labels.push(date_fmt_french);
}
const datapoints = [/*BEGIN-DATAPOINTS*/0, 20, 20, 60, 60, 120, 20, 180, 120, 125, 105, 110, 170/*END-DATAPOINTS*/];
const data = {
    labels: labels,
    datasets: [
        {
            label: 'Machines accessibles',
            data: datapoints,
            borderColor: '#DC6ACF',
            fill: false,
            tension: 0.3
        }
    ]
};

const config = {
    type: 'line',
    data: data,
    options: {
        responsive: true,
        plugins: {
            title: {
                display: true,
                font: {
                    size: 20
                },
                text: 'Statistiques des 7 derniers jours',
                color: 'white'
            },
        },
        interaction: {
            intersect: false,
        },
        scales: {
            x: {
                display: true,
            },
            y: {
                display: true,
                suggestedMin: 0,
                suggestedMax: /*BEGIN-MAX-UP-COUNT*/200/*END-MAX-UP-COUNT*/
            }
        }
    },
};

Chart.defaults.color = '#b5b5b5';
var chart = new Chart(
    document.getElementById('summary-chart-canvas'),
    config
);

// Scroll animations
const observer = new IntersectionObserver(entries => {
    entries.forEach(entry => {
        if (entry.isIntersecting) {
            entry.target.classList.add('show');
            entry.target.classList.remove('hidden');
        } //else {
        //    entry.target.classList.remove('show');
        //    entry.target.classList.add('hidden');
        //}
    });
});
const hiddenElements = document.querySelectorAll('.hidden');
hiddenElements.forEach((el) => observer.observe(el));
