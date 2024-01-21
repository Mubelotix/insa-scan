const DATA_COUNT = 12;
const labels = [];
for (let i = 0; i < DATA_COUNT; ++i) {
    labels.push(i.toString());
}
const datapoints = [0, 20, 20, 60, 60, 120, 20, 180, 120, 125, 105, 110, 170];
const data = {
    labels: labels,
    datasets: [
        {
            label: 'Reachable computers',
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
                text: 'Statistics over the last 30 days',
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
                suggestedMax: 200
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
